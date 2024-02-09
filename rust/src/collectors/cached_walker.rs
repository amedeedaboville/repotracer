// use gix::index::Entry;
use gix::object::tree::EntryRef;
use gix::objs::tree::{EntryKind, EntryMode};
use gix::revision::walk::Info;
use indicatif::{ParallelProgressIterator, ProgressIterator};
use rayon::iter::{IntoParallelRefIterator, ParallelBridge, ParallelIterator};
// use rayon::prelude::*;
use thread_local::ThreadLocal;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use gix::{ObjectId, Repository, Tree};

use std::fmt::Debug;

use crate::stats::common::{
    Either, FileMeasurement, PathMeasurement, TreeDataCollection, TreeReducer,
};

fn count_commits(repo: &Repository) -> Result<usize, Box<dyn std::error::Error>> {
    Ok(repo
        .rev_walk(repo.head_id())
        .first_parent_only()
        .all()?
        .count())
}

pub trait BlobMeasurer<F>: Sync
where
    F: Send + Sync,
{
    fn measure_entry(
        &self,
        repo: &Repository,
        path: &str,
        entry: &EntryRef,
    ) -> Result<F, Box<dyn std::error::Error>>;

    fn measure_data(&self, contents: &[u8]) -> Result<F, Box<dyn std::error::Error>>;
}

pub struct FileContentsMeasurer<F> {
    pub callback: Box<dyn FileMeasurement<F>>,
}
impl<F> BlobMeasurer<F> for FileContentsMeasurer<F>
where
    F: Send + Sync,
{
    fn measure_entry(
        &self,
        repo: &Repository,
        _path: &str,
        entry: &EntryRef,
    ) -> Result<F, Box<dyn std::error::Error>> {
        let obj = entry.object().unwrap();
        let blob = obj.into_blob();
        let content = std::str::from_utf8(&blob.data).unwrap_or_default();
        self.callback.measure_file(repo, "", content)
    }
    fn measure_data(&self, contents: &[u8]) -> Result<F, Box<dyn std::error::Error>> {
        self.callback.measure_bytes(contents)
    }
}
pub struct FilePathMeasurer<F> {
    pub callback: Box<dyn PathMeasurement<F> + Sync>, // Add + Sync here
}

impl<F> BlobMeasurer<F> for FilePathMeasurer<F>
where
    F: Send + Sync,
{
    fn measure_entry(
        &self,
        repo: &Repository,
        path: &str,
        entry: &EntryRef,
    ) -> Result<F, Box<dyn std::error::Error>> {
        self.callback.measure_path(repo, &path)
    }
    fn measure_data(&self, _contents: &[u8]) -> Result<F, Box<dyn std::error::Error>> {
        unimplemented!()
    }
}
pub struct CommitStat {
    pub oid: ObjectId,
    pub stats: Box<dyn Debug>,
}
pub struct CachedWalker<T, F> {
    cache: HashMap<ObjectId, Either<T, F>>,
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
            cache: HashMap::new(),
            repo: gix::open(&repo_path).unwrap(),
            repo_path,
            file_measurer,
            tree_reducer,
        }
    }
    pub fn walk_repo_and_collect_stats(
        &mut self,
        batch_objects: bool,
    ) -> Result<Vec<CommitStat>, Box<dyn std::error::Error>> {
        if batch_objects {
            self.fill_cache();
        }

        let commit_count = count_commits(&gix::open(&self.repo_path)?)?;
        println!("Found {commit_count} commits. Starting to walk the repo.");
        let inner_repo = gix::open(&self.repo_path)?;
        let head_id = inner_repo.head_id();
        let revwalk = inner_repo.rev_walk(head_id).first_parent_only().all()?;

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
            // let inner_repo = Repository::open(self.repo.path())?;
            let (res, processed) = self.measure_tree("", tree, &inner_repo).unwrap();
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
        // for (oid, stats) in commit_stats.iter() {
        //     // println!("{oid}, {:?}", stats);
        // }
        Ok(commit_stats)
    }
    fn fill_cache(&mut self) {
        let repo = gix::open(&self.repo_path).unwrap();
        println!("Processing all files in the object database");
        let start_time = Instant::now();

        let oids = repo
            .objects
            .iter()
            .unwrap()
            .with_ordering(
                gix::odb::store::iter::Ordering::PackAscendingOffsetThenLooseLexicographical,
            )
            .filter_map(|oid_res| oid_res.ok())
            .collect::<Vec<ObjectId>>();
        println!("done collecint oids.");
        let style = ProgressStyle::default_bar().template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {per_sec}").expect("error with progress bar style");
        let progress = ProgressBar::new(oids.len() as u64);
        progress.set_style(style);
        let tl = ThreadLocal::new();
        let path = self.repo_path.clone();
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
                let repo: &Repository =
                    tl.get_or(|| gix::open(&path).expect("Failed to open repository"));
                let obj = repo.find_object(oid).expect("Failed to find object");
                match obj.kind {
                    gix::object::Kind::Blob => {
                        let res = self.file_measurer.measure_data(&obj.data);
                        if res.is_err() {
                            //todo print a warning here
                            return None;
                        } else {
                            Some((oid, Either::Right(res.unwrap())))
                        }
                    }
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
        tree: Tree,
        repo: &Repository,
    ) -> Result<(T, usize), Box<dyn std::error::Error>> {
        if self.cache.contains_key(&tree.id) {
            return Ok((self.cache.get(&tree.id).unwrap().clone().unwrap_left(), 0));
        }
        let mut acc = 0;
        let child_results = tree
            .iter()
            .filter_map(|entry_ref| {
                let Ok(entry) = entry_ref else {
                    return None
                };
                let entry_name = entry.filename().to_string();
                if self.cache.contains_key(&entry.object_id()) {
                    return Some((entry_name, self.cache.get(&entry.object_id()).unwrap().clone()));
                }
                match entry.mode().into() {
                    EntryKind::Tree => {
                        let child_object = entry.object().unwrap();
                        let (child_result, processed) = self
                            .measure_tree(
                                &format!("{path}/{entry_name}"),
                                child_object.into_tree(),
                                repo,
                            )
                            .unwrap();

                        let r = Either::Left(child_result);
                        acc += processed;
                        Some((entry_name, r))
                    }
                    EntryKind::Blob| EntryKind::BlobExecutable => {
                        acc += 1;
                        let child_result = self
                            .file_measurer
                            .measure_entry(repo, path, &entry)
                            .map(Either::Right)
                            .unwrap();
                        Some((entry_name, child_result))
                    }
                    EntryKind::Commit => {
                        println!("Warning: skipping submodule commit '{}' in tree '{entry_name}'", tree.id());
                        None
                    }
                    _  => {
                        println!("Warning: skiping unsupported git object type {:?} under '{entry_name}' in tree {}", entry.mode(), tree.id());
                        None
                    }
                }
            })
            .collect::<TreeDataCollection<T, F>>();
        let res = self.tree_reducer.reduce(repo, child_results)?;
        self.cache.insert(tree.id, Either::Left(res.clone()));
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
