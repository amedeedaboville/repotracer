use gix::filter::plumbing::encoding::mem;
use gix::object::tree::EntryRef;
use gix::objs::tree::{Entry, EntryKind, EntryMode};
use gix::revision::walk::Info;
use indicatif::{ParallelProgressIterator, ProgressIterator};
use polars::datatypes::Flat;
use rayon::iter::{IntoParallelRefIterator, ParallelBridge, ParallelIterator};
use rayon::prelude::*;
use std::str::FromStr;
use thread_local::ThreadLocal;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use smallvec::SmallVec;
use std::borrow::Borrow;
use std::cell::RefCell;
use std::hash::Hash;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use gix::{ObjectId, Repository, Tree};

use std::fmt::Debug;

use crate::stats::common::{
    BlobMeasurer, Either, FileMeasurement, PathMeasurement, TreeDataCollection, TreeReducer,
};
#[derive(Debug)]
pub struct CommitStat {
    pub oid: ObjectId,
    pub stats: Box<dyn Debug>,
}
fn count_commits(repo: &Repository) -> Result<usize, Box<dyn std::error::Error>> {
    Ok(repo
        .rev_walk(repo.head_id())
        .first_parent_only()
        .all()?
        .count())
}

type FlatGitRepo = HashMap<ObjectId, TreeChild>;
pub struct InMemoryGitRepo {
    pub entries: FlatGitRepo,
}
#[derive(Debug)]
pub enum TreeChild {
    Blob,
    Tree(TreeEntry),
}
impl TreeChild {
    pub fn unwrap_tree(&self) -> &TreeEntry {
        match self {
            TreeChild::Tree(t) => t,
            _ => panic!("Called unwrap_tree on TreeChild::Blob"),
        }
    }
}

#[derive(Debug)]
pub enum TreeChildKind {
    Blob,
    Tree,
}
#[derive(Debug)]
pub struct TreeEntry {
    pub oid: ObjectId,
    pub children: Vec<(String, ObjectId, TreeChildKind)>,
}
pub struct CachedWalker<T, F> {
    cache: HashMap<ObjectId, Either<T, F>>,
    filename_cache: HashMap<ObjectId, SmallVec<[String; 2]>>,
    repo_path: String,
    repo: Repository,
    file_measurer: Box<dyn BlobMeasurer<F>>,
    tree_reducer: Box<dyn TreeReducer<T, F>>,
}
impl<T, F> CachedWalker<T, F>
where
    T: Debug + Clone + Send + Sync + 'static,
    F: Debug + Clone + Send + Sync + 'static,
{
    pub fn new(
        repo_path: String,
        file_measurer: Box<dyn BlobMeasurer<F>>, // Changed type here
        tree_reducer: Box<dyn TreeReducer<T, F>>,
    ) -> Self {
        CachedWalker {
            filename_cache: HashMap::new(),
            cache: HashMap::new(),
            repo: gix::open(&repo_path).unwrap(),
            repo_path,
            file_measurer,
            tree_reducer,
        }
    }

    pub fn build_in_memory_tree(&self) -> FlatGitRepo {
        println!("Begining building in memory tree.");
        let repo = gix::open(&self.repo_path).unwrap();
        let oids = repo
            .objects
            .iter()
            .unwrap()
            .filter_map(|oid_res| oid_res.ok())
            .collect::<Vec<ObjectId>>();
        let style = ProgressStyle::default_bar().template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {per_sec}").expect("error with progress bar style");
        let progress = ProgressBar::new(oids.len() as u64);
        progress.set_style(style);
        let shared_repo = gix::ThreadSafeRepository::open(self.repo_path.clone()).unwrap();
        let tl = ThreadLocal::new();
        let oid_entries = repo
            .objects
            .iter()
            .unwrap()
            .with_ordering(
                gix::odb::store::iter::Ordering::PackAscendingOffsetThenLooseLexicographical,
            )
            .progress_with(progress)
            .par_bridge()
            .flat_map(|oid_res| {
                let Ok(oid) = oid_res else { return vec![] };
                let repo: &Repository = tl.get_or(|| shared_repo.clone().to_thread_local());
                let obj = repo.find_object(oid).expect("Failed to find object");
                if oid == ObjectId::from_str("29642cd01b16817f9e53fddb8adc438c0572a866").unwrap() {
                    println!("found oid 29642cd01b16817f9e53fddb8adc438c0572a866");
                    println!("{:?}", obj.kind)
                }
                match obj.kind {
                    gix::object::Kind::Blob => vec![(oid, TreeChild::Blob)],
                    gix::object::Kind::Tree => obj
                        .into_tree()
                        .decode()
                        .unwrap()
                        .entries
                        .iter()
                        .map(|entry| {
                            let filename = entry.filename.to_string();
                            let child_oid: ObjectId = entry.oid.into();
                            (
                                oid,
                                TreeChild::Tree(TreeEntry {
                                    oid,
                                    children: vec![(
                                        filename,
                                        child_oid,
                                        match entry.mode.into() {
                                            EntryKind::Blob => TreeChildKind::Blob,
                                            EntryKind::Tree => TreeChildKind::Tree,
                                            _ => TreeChildKind::Blob,
                                        },
                                    )],
                                }),
                            )
                        })
                        .collect::<Vec<(ObjectId, TreeChild)>>(),
                    gix::object::Kind::Commit => vec![],
                    _ => vec![],
                }
            })
            .fold(
                || HashMap::new(),
                |mut acc: HashMap<ObjectId, TreeChild>, (oid, child)| {
                    acc.insert(oid, child);
                    acc
                },
            )
            .reduce(
                || HashMap::new(),
                |mut acc, cur| {
                    acc.extend(cur);
                    acc
                },
            );
        println!("done grouping tree entries into flat hashamp");
        oid_entries
    }
    pub fn walk_repo_and_collect_stats(
        &mut self,
        batch_objects: bool,
    ) -> Result<Vec<CommitStat>, Box<dyn std::error::Error>> {
        let mut inner_repo = gix::open(&self.repo_path)?;
        inner_repo.object_cache_size(50_000_000);
        if batch_objects {
            self.fill_cache_content_only(&inner_repo);
        }
        let start_time = Instant::now();
        let mem_tree = self.build_in_memory_tree();
        println!(
            "Done building in memory tree in {} seconds",
            start_time.elapsed().as_secs()
        );
        let commit_count = count_commits(&gix::open(&self.repo_path)?)?;
        println!("Found {commit_count} commits. Starting to walk the repo.");
        let head_id = inner_repo.head_id();
        let revwalk = inner_repo
            .rev_walk(head_id)
            .first_parent_only()
            .use_commit_graph(true)
            .all()?;

        let pb = ProgressBar::new(commit_count as u64);
        if commit_count <= 10_000 {
            pb.set_draw_target(ProgressDrawTarget::hidden());
        } else {
            pb.set_style(ProgressStyle::default_bar().template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta})  {msg}",
        ).expect("Failed to set progress bar style"));
        };
        let progress_bar = Some(pb);
        //     self.collect_stats_for_commits(revwalk.all()?, &inner_repo, Some(progress_bar))
        // }

        // pub fn collect_stats_for_commits(
        //     &mut self,
        //     revwalk: gix::revision::Walk, //impl Iterator<Item = Result<gix::oid, Box<dyn std::error::Error>>>,
        //     inner_repo: &Repository,
        //     progress_bar: Option<ProgressBar>,
        // ) -> Result<Vec<CommitStat>, Box<dyn std::error::Error>> {
        let mut i = 0;
        let mut commit_stats = vec![];
        let start_time = Instant::now();
        let alpha = 0.3; // Weighting factor for the EWMA
        let mut ewma_elapsed = Duration::new(0, 0); // Start with an EWMA elapsed time of 0
        let mut last_batch_start = Instant::now(); // Time when the last batch started
        let batch_size = 100; // Update the progress bar every 10 iterations

        let mut objects_procssed = 0;
        let mut objects_procssed_total = 0;
        for result_info in revwalk {
            if result_info.is_err() {
                let e = result_info.unwrap_err();
                println!("Error with commit: {:?}", e);
                continue;
            }
            let info = result_info.unwrap();
            let Info { id: oid, .. } = info;
            i += 1;

            if progress_bar.is_some() && i % batch_size == 0 && i > 0 {
                let progress_bar = progress_bar.as_ref().unwrap();
                let current_batch_duration = last_batch_start.elapsed();
                // Convert current_batch_duration and ewma_elapsed to a common unit (e.g., seconds) for EWMA calculation
                let current_batch_secs = current_batch_duration.as_secs_f64();
                let ewma_secs = ewma_elapsed.as_secs_f64();
                let ewma_secs_updated = alpha * current_batch_secs + (1.0 - alpha) * ewma_secs;
                ewma_elapsed = Duration::from_secs_f64(ewma_secs_updated);

                if ewma_elapsed.as_secs_f64() > 0.0 {
                    // Ensure ewma_elapsed is not zero
                    let ewma_ips = batch_size as f64 / ewma_elapsed.as_secs_f64(); // Calculate iterations per second
                    progress_bar
                        .set_message(format!("{:.2} it/s, {:}", ewma_ips, objects_procssed));
                    objects_procssed = 0
                }

                last_batch_start = Instant::now(); // Reset the start time for the next batch
                progress_bar.inc(batch_size);
            }

            let tree = info.object().unwrap().tree().unwrap();
            // println!("mem_tree has {} entries", mem_tree.len());
            // println!("{:?}", mem_tree.iter().take(5).collect::<Vec<_>>());
            // println!(
            //     "looking up value {} in mem_tree: {:?}",
            //     tree.id,
            //     mem_tree.get(&tree.id)
            // );
            let tree_lookedup = mem_tree.get(&tree.id).unwrap();
            // let inner_repo = Repository::open(self.repo.path())?;
            let (res, processed) = self
                .measure_tree("", tree_lookedup.unwrap_tree(), &inner_repo, &mem_tree)
                .unwrap();
            objects_procssed += processed;
            objects_procssed_total += processed;
            commit_stats.push(CommitStat {
                oid,
                stats: Box::new(res.clone()),
            });
        }
        let elapsed_secs = start_time.elapsed().as_secs_f64();
        if elapsed_secs > 0.0 && objects_procssed_total > 0 {
            println!(
                "processed: {i} commits and {objects_procssed_total} objects in {elapsed_secs}, {:.2} objects/sec",
                objects_procssed_total as f64 / elapsed_secs
            );
        } else {
            println!("processed: {i} commits objects in {elapsed_secs} seconds");
        }
        println!("{:?}", commit_stats.iter().take(5).collect::<Vec<_>>());
        // for stat in &commit_stats {
        //     println!("{:?}", stat);
        // }
        Ok(commit_stats)
    }
    /// Fills the <oid, result> cache, but only for blobs (files)
    /// Only called if the stat collector only need file contents
    ///
    fn fill_cache_content_only(&mut self, repo: &Repository) {
        println!("Processing all files in the object database");
        let start_time = Instant::now();

        let oids = repo
            .objects
            .iter()
            .unwrap()
            .filter_map(|oid_res| oid_res.ok())
            .collect::<Vec<ObjectId>>();
        println!("done collecting oids.");
        let style = ProgressStyle::default_bar().template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {per_sec}").expect("error with progress bar style");
        let progress = ProgressBar::new(oids.len() as u64);
        progress.set_style(style);
        let tl = ThreadLocal::new();
        let path = self.repo_path.clone();
        let shared_repo = gix::ThreadSafeRepository::open(path.clone()).unwrap();
        // if with_filename {
        //     let pb = progress.clone();
        //     self.filename_cache = shared_repo
        //         .objects
        //         .iter()
        //         .unwrap()
        //         .with_ordering(
        //             gix::odb::store::iter::Ordering::PackAscendingOffsetThenLooseLexicographical,
        //         )
        //         .par_bridge()
        //         .progress_with(pb)
        //         .filter_map(|oid_res| {
        //             let Ok(oid) = oid_res else { return None };
        //             let tl_repo: &Repository = tl.get_or(|| gix::open(path.clone()).unwrap());
        //             let obj = tl_repo.find_object(oid).expect("Failed to find object");
        //             match obj.kind {
        //                 gix::object::Kind::Tree => Some(
        //                     obj.into_tree()
        //                         .iter()
        //                         .filter_map(|entry| {
        //                             if entry.is_err() {
        //                                 return None;
        //                             }
        //                             let entry = entry.unwrap();
        //                             if !entry.mode().is_blob() {
        //                                 return None;
        //                             }
        //                             let name = entry.filename().to_string();
        //                             Some((entry.object_id().clone(), name))
        //                         })
        //                         .collect::<Vec<(gix::ObjectId, String)>>(),
        //                 ),
        //                 _ => None,
        //             }
        //         })
        //         // .collect::<HashMap<ObjectId, >>();
        //         .flatten()
        //         .fold(
        //             || HashMap::new(),
        //             |mut acc: HashMap<gix::ObjectId, SmallVec<[String; 2]>>, (oid, name)| {
        //                 acc.entry(oid).or_insert_with(SmallVec::new).push(name);
        //                 acc
        //             },
        //         )
        //         .reduce(
        //             || HashMap::new(),
        //             |mut acc, cur| {
        //                 acc.extend(cur);
        //                 acc
        //             },
        //         );
        //     // .reduce_with(|mut acc, cur| {
        //     //     for (name, oid) in cur {
        //     //         acc.entry(oid).or_insert_with(SmallVec::new).push(name);
        //     //     }
        //     //     acc
        //     // })
        //     // .unwrap_or_default();
        //     println!(
        //         "Count {} filenames in {} seconds",
        //         self.cache.len(),
        //         start_time.elapsed().as_secs()
        //     );
        // }
        self.cache = repo
            .objects
            .iter()
            .unwrap()
            .with_ordering(
                gix::odb::store::iter::Ordering::PackAscendingOffsetThenLooseLexicographical,
            )
            .par_bridge()
            .progress_with(progress)
            .filter_map(|oid_res| {
                let Ok(oid) = oid_res else { return None };
                let repo: &Repository = tl.get_or(|| shared_repo.clone().to_thread_local());
                let obj = repo.find_object(oid).expect("Failed to find object");
                match obj.kind {
                    gix::object::Kind::Blob => self
                        .file_measurer
                        .measure_data(&obj.data)
                        .ok()
                        .map(|r| (oid, Either::Right(r))),
                    _ => None,
                }
            })
            .collect::<HashMap<ObjectId, Either<T, F>>>();

        println!(
            "Processed {} blobs (files) in {} seconds",
            self.cache.len(),
            start_time.elapsed().as_secs()
        );
    }
    fn fill_cache_including_filename(&mut self, repo: &Repository) {
        println!("Processing all files in the object database");
        let start_time = Instant::now();

        let oids = repo
            .objects
            .iter()
            .unwrap()
            .filter_map(|oid_res| oid_res.ok())
            .collect::<Vec<ObjectId>>();
        println!("done collecting oids.");
        let style = ProgressStyle::default_bar().template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {per_sec}").expect("error with progress bar style");
        let progress = ProgressBar::new(oids.len() as u64);
        progress.set_style(style);
        let tl = ThreadLocal::new();
        let path = self.repo_path.clone();
        let shared_repo = gix::ThreadSafeRepository::open(path.clone()).unwrap();
        // if with_filename {
        //     let pb = progress.clone();
        //     self.filename_cache = shared_repo
        //         .objects
        //         .iter()
        //         .unwrap()
        //         .with_ordering(
        //             gix::odb::store::iter::Ordering::PackAscendingOffsetThenLooseLexicographical,
        //         )
        //         .par_bridge()
        //         .progress_with(pb)
        //         .filter_map(|oid_res| {
        //             let Ok(oid) = oid_res else { return None };
        //             let tl_repo: &Repository = tl.get_or(|| gix::open(path.clone()).unwrap());
        //             let obj = tl_repo.find_object(oid).expect("Failed to find object");
        //             match obj.kind {
        //                 gix::object::Kind::Tree => Some(
        //                     obj.into_tree()
        //                         .iter()
        //                         .filter_map(|entry| {
        //                             if entry.is_err() {
        //                                 return None;
        //                             }
        //                             let entry = entry.unwrap();
        //                             if !entry.mode().is_blob() {
        //                                 return None;
        //                             }
        //                             let name = entry.filename().to_string();
        //                             Some((entry.object_id().clone(), name))
        //                         })
        //                         .collect::<Vec<(gix::ObjectId, String)>>(),
        //                 ),
        //                 _ => None,
        //             }
        //         })
        //         // .collect::<HashMap<ObjectId, >>();
        //         .flatten()
        //         .fold(
        //             || HashMap::new(),
        //             |mut acc: HashMap<gix::ObjectId, SmallVec<[String; 2]>>, (oid, name)| {
        //                 acc.entry(oid).or_insert_with(SmallVec::new).push(name);
        //                 acc
        //             },
        //         )
        //         .reduce(
        //             || HashMap::new(),
        //             |mut acc, cur| {
        //                 acc.extend(cur);
        //                 acc
        //             },
        //         );
        //     // .reduce_with(|mut acc, cur| {
        //     //     for (name, oid) in cur {
        //     //         acc.entry(oid).or_insert_with(SmallVec::new).push(name);
        //     //     }
        //     //     acc
        //     // })
        //     // .unwrap_or_default();
        //     println!(
        //         "Count {} filenames in {} seconds",
        //         self.cache.len(),
        //         start_time.elapsed().as_secs()
        //     );
        // }
        self.cache = repo
            .objects
            .iter()
            .unwrap()
            .with_ordering(
                gix::odb::store::iter::Ordering::PackAscendingOffsetThenLooseLexicographical,
            )
            .par_bridge()
            .progress_with(progress)
            .filter_map(|oid_res| {
                let Ok(oid) = oid_res else { return None };
                let repo: &Repository = tl.get_or(|| shared_repo.clone().to_thread_local());
                let obj = repo.find_object(oid).expect("Failed to find object");
                match obj.kind {
                    gix::object::Kind::Blob => self
                        .file_measurer
                        .measure_data(&obj.data)
                        .ok()
                        .map(|r| (oid, Either::Right(r))),
                    _ => None,
                }
            })
            .collect::<HashMap<ObjectId, Either<T, F>>>();

        println!(
            "Processed {} blobs (files) in {} seconds",
            self.cache.len(),
            start_time.elapsed().as_secs()
        );
    }
    fn measure_tree(
        &mut self,
        path: &str,
        tree: &TreeEntry,
        repo: &Repository,
        mem_tree: &FlatGitRepo,
    ) -> Result<(T, usize), Box<dyn std::error::Error>> {
        if self.cache.contains_key(&tree.oid) {
            return Ok((self.cache.get(&tree.oid).unwrap().clone().unwrap_left(), 0));
        }
        let mut acc = 0;
        let child_results = tree
            .children
            .iter()
            .filter_map(|entry| {
                let (entry_str, oid, kind) = entry;
                let entry_name = entry_str.clone();
                if self.cache.contains_key(oid) {
                    return Some((entry_name, self.cache.get(oid).unwrap().clone()));
                }
                match kind {
                    TreeChildKind::Blob => {
                        acc += 1;
                        match self
                            .file_measurer
                            .measure_entry(repo, path, oid) {
                                Ok(measurement) => Some((entry_name, Either::Right(measurement))),
                                Err(_) => None,
                            }
                    }
                    TreeChildKind::Tree => {
                        let child_object = mem_tree.get(oid).unwrap();
                        let (child_result, processed) = self
                            .measure_tree(
                                &format!("{path}/{entry_name}"),
                                child_object.unwrap_tree(),
                                repo,
                                mem_tree,
                            )
                            .unwrap();

                        let r = Either::Left(child_result);
                        acc += processed;
                        Some((entry_name, r))
                    }
                    _  => {
                        println!("Warning: skiping unsupported git object type {:?} under '{entry_name}' in tree {}", kind, oid);
                        None
                    }
                }
            })
            .collect::<TreeDataCollection<T, F>>();
        let res = self.tree_reducer.reduce(repo, child_results)?;
        self.cache.insert(tree.oid, Either::Left(res.clone()));
        Ok((res, acc))
    }
}

enum Granularity {
    Daily,
    Hourly,
    EveryXHours(u32),
    Weekly,
    Monthly,
}
