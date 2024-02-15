use bincode::{deserialize, serialize};
use flate2::read::{GzDecoder, ZlibDecoder};
use gix::objs::tree::{Entry, EntryKind, EntryMode};
use gix::objs::Kind;
use gix::revision::walk::Info;
use indexmap::IndexSet;
use indicatif::{ParallelProgressIterator, ProgressIterator};
use rayon::iter::{IntoParallelRefIterator, ParallelBridge, ParallelIterator};
use rayon::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use smallset::SmallSet;
use std::any::Any;
use std::collections::HashSet;
use std::fs::File;
use std::io::BufWriter;
use std::io::{self, BufReader};
use std::str::FromStr;
use thread_local::ThreadLocal;

use ahash::{AHashMap, AHashSet};
use gix::{ObjectId, Repository, ThreadSafeRepository, Tree};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use flate2::{write::ZlibEncoder, Compression};
use gzp::{deflate::Gzip, ZBuilder, ZWriter};
use std::fmt::Debug;
use std::io::Write;

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

type MyOid = usize;
type FlatGitRepo = AHashMap<MyOid, TreeChild>;
type FilenameSet = IndexSet<String>;
type FilenameIdx = usize;
type FilenameCache = AHashMap<MyOid, HashSet<FilenameIdx>>;
type OidSet = IndexSet<(ObjectId, Kind)>;
type EntrySet = IndexSet<MyEntry>;
type EntryIdx = u32;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct MyEntry {
    pub oid_idx: MyOid,
    pub filename_idx: FilenameIdx,
    pub kind: TreeChildKind,
}
pub struct RepoCachedInfo {
    pub flat_tree: FlatGitRepo,
    pub filename_cache: FilenameCache,
    pub filenames: FilenameSet,
}
#[derive(Debug, Serialize, Deserialize, Clone)]

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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub enum TreeChildKind {
    Blob,
    Tree,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TreeEntry {
    pub oid: MyOid,
    pub children: Vec<EntryIdx>,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BlobParent {
    pub tree_id: usize,
    pub filename_idx: usize,
}

pub struct CachedWalker<T, F> {
    cache: AHashMap<(MyOid, Option<FilenameIdx>), Either<T, F>>,
    filename_cache: FilenameCache,
    mem_tree: FlatGitRepo,
    repo_path: String,
    repo: Repository,
    file_measurer: Box<dyn BlobMeasurer<F>>,
    tree_reducer: Box<dyn TreeReducer<T, F> + Sync + Send>, // Going to use this across threads
}
impl<T, F> CachedWalker<T, F>
where
    T: Debug + Clone + Send + Sync + 'static,
    F: Debug + Clone + Send + Sync + 'static,
{
    pub fn new(
        repo_path: String,
        file_measurer: Box<dyn BlobMeasurer<F>>, // Changed type here
        tree_reducer: Box<dyn TreeReducer<T, F> + Sync + Send>, // Adjusted type here
    ) -> Self {
        CachedWalker {
            filename_cache: AHashMap::new(),
            cache: AHashMap::new(),
            repo: gix::open(&repo_path).unwrap(),
            mem_tree: AHashMap::new(),
            repo_path,
            file_measurer,
            tree_reducer,
        }
    }

    pub fn build_filename_set(
        &self,
        repo: &ThreadSafeRepository,
        oid_set: &OidSet,
    ) -> Result<FilenameSet, Box<dyn std::error::Error>> {
        let tl = ThreadLocal::new();
        let style = ProgressStyle::default_bar().template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {per_sec}").expect("error with progress bar style");
        let progress = ProgressBar::new(oid_set.len() as u64);
        progress.set_style(style);
        let filenames: FilenameSet = oid_set
            .iter()
            .progress_with(progress)
            .par_bridge()
            .fold(IndexSet::new, |mut acc: IndexSet<String>, (oid, kind)| {
                let repo: &Repository = tl.get_or(|| repo.clone().to_thread_local());
                if !kind.is_tree() {
                    return acc;
                }
                let obj = repo.find_object(*oid).expect("Failed to find object");
                for entry in obj.into_tree().decode().unwrap().entries.iter() {
                    acc.insert(entry.filename.to_string());
                }
                acc
            })
            .reduce(
                || IndexSet::new(),
                |mut acc: IndexSet<String>, set: IndexSet<String>| {
                    acc.extend(set);
                    acc
                },
            );
        println!(
            "Built filename set with {} unique filenames",
            filenames.len()
        );
        Ok(filenames)
    }
    pub fn build_entries_set(
        &self,
        repo: &ThreadSafeRepository,
        oid_set: &OidSet,
        filename_set: &FilenameSet,
    ) -> Result<EntrySet, Box<dyn std::error::Error>> {
        let num_oids = oid_set.len() as u64;
        let tl = ThreadLocal::new();
        let style = ProgressStyle::default_bar().template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {per_sec}").expect("error with progress bar style");
        let progress = ProgressBar::new(oid_set.len() as u64);
        progress.set_style(style);
        let entries = oid_set
            .iter()
            .par_bridge()
            .progress_with(progress)
            .fold(IndexSet::new, |mut acc: EntrySet, (oid, kind)| {
                let repo: &Repository = tl.get_or(|| repo.clone().to_thread_local());
                if !kind.is_tree() {
                    return acc;
                }
                let obj = repo.find_object(*oid).expect("Failed to find object");
                for entry in obj.into_tree().decode().unwrap().entries.iter() {
                    let child_oid: ObjectId = entry.oid.into();
                    let filename_idx = filename_set
                        .get_index_of(&entry.filename.to_string())
                        .unwrap();
                    let kind = match entry.mode.into() {
                        EntryKind::Blob => TreeChildKind::Blob,
                        EntryKind::Tree => TreeChildKind::Tree,
                        _ => continue,
                    };
                    let other_kind =
                        match entry.mode.into() {
                            EntryKind::Blob => Kind::Blob,
                            EntryKind::Tree => Kind::Tree,
                            _ => {
                                println!("unreachable new code");
                                continue;
                            }
                        };
                    let child_oid_idx = oid_set.get_index_of(&(child_oid, other_kind)).unwrap();
                    acc.insert(MyEntry {
                        oid_idx: child_oid_idx,
                        filename_idx,
                        kind,
                    });
                }
                acc
            })
            .reduce(IndexSet::new, |mut acc, cur| {
                for entry in cur {
                    acc.insert(entry);
                }
                acc
            });
        Ok(entries)
    }

    pub fn build_flat_tree(
        &self,
        repo: &ThreadSafeRepository,
        oid_set: &OidSet,
        entry_set: &EntrySet,
        filenames: &FilenameSet,
    ) -> Result<FlatGitRepo, Box<dyn std::error::Error>> {
        let tl = ThreadLocal::new();
        let style = ProgressStyle::default_bar().template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {per_sec}").expect("error with progress bar style");
        let progress = ProgressBar::new(oid_set.len() as u64);
        progress.set_style(style);
        let oid_entries: AHashMap<MyOid, TreeChild> = oid_set.iter()
            .progress_with(progress)
            .par_bridge()
            .fold(AHashMap::new, |mut acc: FlatGitRepo, (oid, kind) | {
                //todo just use enumerate here
                let oid_idx = oid_set.get_index_of(&(*oid, *kind)).unwrap();
                //Duplicate object
                if acc.contains_key(&oid_idx) { return acc }
                let repo: &Repository = tl.get_or(|| repo.clone().to_thread_local());
                let obj =
                    match repo.find_object(*oid) {
                        Ok(obj) => obj,
                        Err(_) => {println!("Error fetching object {oid}"); return acc},
                    };
                match obj.kind {
                    gix::object::Kind::Tree => {
                        let tree_entry_children = obj
                            .into_tree()
                            .decode()
                            .unwrap()
                            .entries
                            .iter()
                            .filter_map(|entry| {
                                let filename_idx =
                                    filenames.get_index_of(&entry.filename.to_string()).unwrap();
                                let child_oid: ObjectId = entry.oid.into();
                                //todo we gotta standardize of the EnttryKidn enums
                                let child_kind = match entry.mode.into() {
                                    EntryKind::Blob => Kind::Blob,
                                    EntryKind::Tree => Kind::Tree,
                                    _ => return None,
                                };
                                //also indexSet could be an IndexMap of oid->kind
                                let child_oid_idx = oid_set.get_index_of(&(child_oid, child_kind)).unwrap();
                                let kind = match entry.mode.into() {
                                    EntryKind::Blob => TreeChildKind::Blob,
                                    EntryKind::Tree => TreeChildKind::Tree,
                                    _ => return None,
                                };
                                let tree_entry = MyEntry {oid_idx: child_oid_idx,filename_idx,  kind};
                                let entry_idx = entry_set.get_index_of(&tree_entry).unwrap() as EntryIdx;
                                if child_oid
                                    == ObjectId::from_str(
                                        "00000264228858f73a003e22cb157df1634519cb".into(),
                                    )
                                    .unwrap()
                                {
                                    println!(
                                        "Processing tree entry for child {:?} (child_idx {}) (parent tree {} idx {}) with mode {:?}. Has entry id {}",
                                        child_oid,
                                        child_oid_idx,
                                        oid,
                                        oid_idx,
                                        entry.mode.as_str(),
                                        entry_idx
                                    );
                                }
                                Some(entry_idx)
                            })
                            .collect::<Vec<EntryIdx>>();
                        acc.insert(
                            oid_idx,
                            TreeChild::Tree(TreeEntry {
                                oid: oid_idx,
                                children: tree_entry_children,
                            }),
                        );
                    }
                    _ => {}
                }
                acc
            })
            .reduce(AHashMap::new, |mut acc, cur| {
                acc.extend(cur);
                acc
            });
        Ok(oid_entries)
    }
    pub fn build_oid_set(&self, repo: &Repository) -> Result<OidSet, Box<dyn std::error::Error>> {
        let oids = repo
            .objects
            .iter()
            .unwrap()
            .with_ordering(
                gix::odb::store::iter::Ordering::PackAscendingOffsetThenLooseLexicographical,
            )
            .filter_map(|oid_res| {
                oid_res
                    .map(|oid| (oid, repo.find_header(oid).unwrap().kind()))
                    .ok()
            })
            .collect::<Vec<(ObjectId, Kind)>>();
        let oid_set = oids.into_iter().collect::<IndexSet<(ObjectId, Kind)>>();
        Ok(oid_set)
    }
    pub fn build_in_memory_tree(
        &self,
    ) -> (
        bool,
        FlatGitRepo,
        FilenameCache,
        FilenameSet,
        OidSet,
        EntrySet,
    ) {
        println!("Begining building in memory tree.");
        let repo = gix::open(&self.repo_path).unwrap();
        let oid_set: OidSet = match self.load_cache::<OidSet>("oids") {
            Ok(f) => {
                println!("Loaded {} objects from cache.", f.len());
                f
            }
            Err(e) => {
                println!("Computing object set from scratch due to error: {}", e);
                let oids = self.build_oid_set(&repo).unwrap();
                self.save_cache("oids", &oids);
                oids
            }
        };
        println!("Found {} oids", oid_set.len());
        let shared_repo = gix::ThreadSafeRepository::open(self.repo_path.clone()).unwrap();

        let filenames: FilenameSet =
            match self.load_cache::<FilenameSet>("filenames") {
                Ok(f) => {
                    println!("Loaded {} filenames from cache.", f.len());
                    f
                }
                Err(e) => {
                    println!("Computing filenames from scratch due to error: {}", e);
                    let filenames = self.build_filename_set(&shared_repo, &oid_set).unwrap();
                    self.save_cache("filenames", &filenames);
                    filenames
                }
            };
        println!("Startig to build in memory tree");
        let entries = match self.load_cache::<EntrySet>("entries") {
            Ok(f) => {
                println!("Loaded {} tree entries from cache.", f.len());
                f
            }
            Err(e) => {
                println!("Computing tree entries from scratch due to error: {}", e);
                let entries = self
                    .build_entries_set(&shared_repo, &oid_set, &filenames)
                    .unwrap();
                self.save_cache("entries", &entries);
                entries
            }
        };
        println!("Getting flat AHashMap of all trees in the repo.");
        let oid_entries = match self.load_cache::<FlatGitRepo>("flat_tree") {
            Ok(f) => {
                println!("Loaded {} trees from cache.", f.len());
                f
            }
            Err(e) => {
                println!("Flattening trees from scratch due to error: {}", e);
                let entries = self
                    .build_flat_tree(&shared_repo, &oid_set, &entries, &filenames)
                    .unwrap();
                self.save_cache("flat_tree", &entries);
                entries
            }
        };
        let filename_cache = self
            .build_filename_cache(&entries, &oid_set, &oid_entries)
            .unwrap();

        (
            false,
            oid_entries,
            filename_cache,
            filenames,
            oid_set,
            entries,
        )
    }
    pub fn build_filename_cache(
        &self,
        entry_set: &EntrySet,
        oid_set: &OidSet,
        flat_repo: &FlatGitRepo,
    ) -> Result<FilenameCache, Box<dyn std::error::Error>> {
        let start = Instant::now();
        let style = ProgressStyle::default_bar().template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {per_sec}").expect("error with progress bar style");
        let progress = ProgressBar::new(entry_set.len() as u64);
        progress.set_style(style);
        let mut filename_cache = entry_set
            .iter()
            .par_bridge()
            .progress_with(progress)
            .fold(AHashMap::new, |mut acc: FilenameCache, entry: &MyEntry| {
                let MyEntry {
                    oid_idx,
                    filename_idx,
                    kind,
                } = entry;
                // let oid = oid_set.get_index(oid_idx).unwrap();
                // let mut debug = false;
                // if *oid == ObjectId::from_str("6e37fc5b0155dd4755770b92be273c9f099a55ef").unwrap() {
                //     println!("Processing tree {}", oid_idx);
                //     debug = true;
                // }
                if *kind != TreeChildKind::Blob {
                    return acc;
                }
                // if debug {
                //     println!("Parent tree has child with entry id {}", entry_idx);
                // }
                let acc_entry = acc.entry(*oid_idx).or_insert_with(HashSet::new);
                // if debug {
                //     println!(
                //         "This entry contains {} {} {:?} ",
                //         filename_idx, oid_idx, child_kind
                //     );
                // }
                acc_entry.insert(*filename_idx);
                acc
            })
            .reduce(
                || AHashMap::with_capacity(flat_repo.len()),
                |mut acc: FilenameCache, cur: FilenameCache| {
                    for (key, value) in cur {
                        // The uncommented code is ~10x faster than this "idiomatic" version
                        // acc.entry(key).or_insert_with(HashSet::new).extend(value);
                        if let Some(existing) = acc.get_mut(&key) {
                            existing.extend(value);
                        } else {
                            acc.insert(key, value);
                        }
                    }
                    acc
                },
            );
        println!(
            "Built filename cache in {} seconds",
            start.elapsed().as_secs()
        );
        // idk if this helps or if the bincode library already does it when loading from file
        for (oid, filename_set) in filename_cache.iter_mut() {
            filename_set.shrink_to_fit();
        }
        Ok(filename_cache)
    }
    pub fn walk_repo_and_collect_stats(
        &mut self,
        batch_objects: bool,
    ) -> Result<Vec<CommitStat>, Box<dyn std::error::Error>> {
        let mut inner_repo = gix::open(&self.repo_path)?;
        inner_repo.object_cache_size(50_000_000);
        let start_time = Instant::now();
        let (loaded_from_file, flat_tree, filename_cache, filename_set, oid_set, entry_set) =
            self.build_in_memory_tree();
        println!(
            "Built in memory tree in {} seconds",
            start_time.elapsed().as_secs()
        );
        if batch_objects {
            self.batch_process_objects(&inner_repo, &filename_cache, &oid_set);
        }
        // if !loaded_from_file {
        //     if let Err(e) = self.save_to_file(&filename_cache, &mem_tree) {
        //         eprintln!("Failed to save to file: {}", e);
        //     }
        // }
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
                    let ewma_ips = batch_size as f64 / ewma_elapsed.as_secs_f64(); // Calculate iterations per second
                    progress_bar
                        .set_message(format!("{:.2} it/s, {:}", ewma_ips, objects_procssed));
                    objects_procssed = 0
                }

                last_batch_start = Instant::now(); // Reset the start time for the next batch
                progress_bar.inc(batch_size);
            }

            let tree = info.object().unwrap().tree().unwrap();
            let tree_alias_idx = oid_set.get_index_of(&(tree.id, Kind::Tree)).unwrap();
            let tree_lookedup = flat_tree.get(&tree_alias_idx).unwrap();
            let (res, processed) = self
                .measure_tree(
                    "",
                    tree_lookedup.unwrap_tree(),
                    &inner_repo,
                    &flat_tree,
                    &filename_set,
                    &oid_set,
                    &entry_set,
                )
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
    fn batch_process_objects(
        &mut self,
        repo: &Repository,
        filename_cache: &FilenameCache,
        oid_set: &OidSet,
    ) {
        println!("Processing all files in the object database");
        let start_time = Instant::now();

        let filename_cache_entries: usize = filename_cache
            .iter()
            .map(|(_oid, filenames)| filenames.iter().count())
            .sum();
        println!(
            "Have {} objects, and {} entries in the filename cache for {} blobs",
            oid_set.len(),
            filename_cache_entries,
            filename_cache.len()
        );
        let style = ProgressStyle::default_bar().template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {per_sec}").expect("error with progress bar style");
        let progress = ProgressBar::new(oid_set.len() as u64);
        progress.set_style(style);
        let tl = ThreadLocal::new();
        let path = self.repo_path.clone();
        let shared_repo = gix::ThreadSafeRepository::open(path.clone()).unwrap();
        self.cache = oid_set
            .iter()
            .par_bridge()
            .progress_with(progress)
            .fold(
                AHashMap::new,
                |mut acc: AHashMap<(MyOid, Option<FilenameIdx>), Either<T, F>>, (oid, kind)| {
                    if !kind.is_blob() {
                        return acc;
                    }
                    let repo: &Repository = tl.get_or(|| shared_repo.clone().to_thread_local());
                    let obj = repo.find_object(*oid).expect("Failed to find object");
                    match obj.kind {
                        gix::object::Kind::Blob => {
                            //todo just use enumerate instead of re-looking up
                            let oid_idx = oid_set.get_index_of(&(*oid, *kind)).unwrap();
                            let Some(parent_trees) = filename_cache.get(&oid_idx) else {
                                // println!(
                                //     "No parent trees in filename cache for blob: {} with idx {}",
                                //     oid, oid_idx
                                // );
                                return acc;
                            };
                            for filename_idx in parent_trees {
                                let parent_filename = "parentfilename".to_owned();
                                let file_res =
                                    self.file_measurer
                                        .measure_entry(repo, &parent_filename, &oid);
                                match file_res {
                                    Ok(measurement) => {
                                        acc.insert(
                                            (oid_idx, Some(*filename_idx)),
                                            Either::Right(measurement),
                                        );
                                    }
                                    Err(_) => {}
                                }
                            }
                        }
                        _ => {}
                    };
                    acc
                },
            )
            .reduce(
                || AHashMap::new(),
                |mut acc: AHashMap<(MyOid, Option<FilenameIdx>), Either<T, F>>, cur| {
                    acc.extend(cur);
                    acc
                },
            );

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
        filename_set: &IndexSet<String>,
        oid_set: &OidSet,
        entry_set: &IndexSet<MyEntry>,
    ) -> Result<(T, usize), Box<dyn std::error::Error>> {
        if self.cache.contains_key(&(tree.oid, None)) {
            return Ok((
                self.cache
                    .get(&(tree.oid, None))
                    .unwrap()
                    .clone()
                    .unwrap_left(),
                0,
            ));
        }
        let mut acc = 0;
        let child_results = tree
            .children
            .iter()
            .filter_map(|entry_idx| {
                let entry = entry_set
                    .get_index(*entry_idx as usize)
                    .expect(&format!("Did not find entry_idx {entry_idx} in entry_set"));
                let MyEntry {
                    oid_idx,
                    filename_idx,
                    kind,
                } = entry;
                let (oid, _) = oid_set
                    .get_index(*oid_idx)
                    .expect(&format!("Did not find {oid_idx} in oid_set"));
                let entry_name = filename_set
                    .get_index(*filename_idx)
                    .expect(&format!("Did not find {filename_idx} in filename_set"));
                let entry_path = format!("{path}/{entry_name}");
                let cache_key_if_folder = &(*oid_idx, None);
                if self.cache.contains_key(cache_key_if_folder) {
                    return Some((
                        entry_path,
                        self.cache
                            .get(cache_key_if_folder)
                            .expect(&format!(
                                "Didn't find result in cahe (folder key) even though it exists"
                            ))
                            .clone(),
                    ));
                }
                let cache_key_if_blob = &(*oid_idx, Some(*filename_idx));
                if self.cache.contains_key(cache_key_if_blob) {
                    return Some((
                        entry_path,
                        self.cache
                            .get(cache_key_if_blob)
                            .expect(&format!("Did not find result in blob cacahe"))
                            .clone(),
                    ));
                }
                match kind {
                    TreeChildKind::Blob => {
                        acc += 1;
                        match self.file_measurer.measure_entry(repo, &entry_path, oid) {
                            Ok(measurement) => Some((entry_path, Either::Right(measurement))),
                            Err(_) => {
                                println!("Measuring blob with oid {oid} failed");
                                None
                            }
                        }
                    }
                    TreeChildKind::Tree => {
                        let child_object = mem_tree
                            .get(oid_idx)
                            .expect("Did not find {oid_idx} in flat repo");
                        let (child_result, processed) = self
                            .measure_tree(
                                &entry_path,
                                child_object.unwrap_tree(),
                                repo,
                                mem_tree,
                                filename_set,
                                oid_set,
                                entry_set,
                            )
                            .expect("Measure tree for {oid_idx} failed");

                        let r = Either::Left(child_result);
                        acc += processed;
                        Some((entry_path, r))
                    }
                }
            })
            .collect::<TreeDataCollection<T, F>>();
        let res = self.tree_reducer.reduce(repo, child_results)?;
        self.cache
            .insert((tree.oid, None), Either::Left(res.clone()));
        Ok((res, acc))
    }

    fn load_cache<Cache>(&self, cache_name: &str) -> io::Result<Cache>
    where
        Cache: DeserializeOwned,
    {
        let start_time = Instant::now();
        let cache_path = format!("{}/{}.bin", self.repo_path, cache_name);
        let cache_file = File::open(cache_path)?;
        let cache_reader = BufReader::new(cache_file);
        let cache: Cache = bincode::deserialize_from(cache_reader)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        println!(
            "Loaded {cache_name} in {} seconds",
            start_time.elapsed().as_secs()
        );
        Ok(cache)
    }
    fn save_cache<Cache>(&self, cache_name: &str, cache: &Cache) -> io::Result<()>
    where
        Cache: Serialize,
    {
        let cache_path = format!("{}/{}.bin", self.repo_path, cache_name);
        let cache_file = File::create(cache_path)?;
        let mut cache_writer = BufWriter::new(cache_file);
        bincode::serialize_into(&mut cache_writer, cache)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(())
    }
}
