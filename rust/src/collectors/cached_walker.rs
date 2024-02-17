use dashmap::DashMap;

use gix::objs::tree::EntryKind;
use gix::objs::Kind;
use gix::revision::walk::Info;
use indexmap::IndexSet;
use indicatif::{ParallelProgressIterator, ProgressIterator};
use rayon::iter::{ParallelBridge, ParallelIterator};
use rayon::prelude::*;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use smallvec::SmallVec;

use std::collections::HashSet;
use std::fs::File;
use std::hash::Hash;
use std::io::BufWriter;
use std::io::{self, BufReader};
use std::str::FromStr;
use thread_local::ThreadLocal;

use ahash::AHashMap;
use gix::{ObjectId, Repository, ThreadSafeRepository};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::time::{Duration, Instant};

use std::fmt::Debug;

use crate::stats::common::{BlobMeasurer, Either, PossiblyEmpty, TreeDataCollection, TreeReducer};

#[derive(Debug)]
pub struct CommitStat {
    pub oid: ObjectId,
    pub stats: Box<dyn Debug>,
}
unsafe impl Send for CommitStat {}
unsafe impl Sync for CommitStat {}
fn count_commits(repo: &Repository) -> Result<usize, Box<dyn std::error::Error>> {
    Ok(repo
        .rev_walk(repo.head_id())
        .first_parent_only()
        .all()?
        .count())
}

type MyOid = u32;
type FlatGitRepo = AHashMap<MyOid, TreeChild>;
type FilenameSet = IndexSet<String>;
type FilenameIdx = u32;
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
    pub oid_idx: MyOid,
    pub children: Vec<EntryIdx>,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BlobParent {
    pub tree_id: usize,
    pub filename_idx: usize,
}

type ResultCache<T, F> = DashMap<(MyOid, Option<FilenameIdx>), Either<T, F>>;
pub struct CachedWalker<T, F> {
    cache: ResultCache<T, F>,
    filename_cache: FilenameCache,
    mem_tree: FlatGitRepo,
    repo_path: String,
    file_measurer: Box<dyn BlobMeasurer<F>>,
    tree_reducer: Box<dyn TreeReducer<T, F> + Sync + Send>, // Going to use this across threads
}
impl<T, F> CachedWalker<T, F>
where
    T: Debug + Clone + Send + Sync + 'static + PossiblyEmpty,
    F: Debug + Clone + Send + Sync + 'static,
{
    pub fn new(
        repo_path: String,
        file_measurer: Box<dyn BlobMeasurer<F>>, // Changed type here
        tree_reducer: Box<dyn TreeReducer<T, F> + Sync + Send>, // Adjusted type here
    ) -> Self {
        CachedWalker {
            filename_cache: AHashMap::new(),
            cache: DashMap::new(),
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
        let start = Instant::now();
        let tl = ThreadLocal::new();
        let style = ProgressStyle::default_bar().template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {per_sec}").expect("error with progress bar style");
        let progress = ProgressBar::new(oid_set.len() as u64);
        progress.set_style(style);
        let filenames: FilenameSet =
            oid_set
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
                    IndexSet::new,
                    |mut acc: IndexSet<String>, set: IndexSet<String>| {
                        acc.extend(set);
                        acc
                    },
                );
        println!(
            "Built filename set with {} unique filenames in {} secs",
            filenames.len(),
            start.elapsed().as_secs_f64()
        );

        Ok(filenames)
    }
    pub fn build_entries_set(
        &self,
        repo: &ThreadSafeRepository,
        oid_set: &OidSet,
        filename_set: &FilenameSet,
    ) -> Result<EntrySet, Box<dyn std::error::Error>> {
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
                        .unwrap() as FilenameIdx;
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
                    let child_oid_idx =
                        oid_set.get_index_of(&(child_oid, other_kind)).unwrap() as MyOid;
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
        let oid_entries: AHashMap<MyOid, TreeChild> =
            oid_set
                .iter()
                .enumerate()
                .progress_with(progress)
                .par_bridge()
                .fold(
                    AHashMap::new,
                    |mut acc: FlatGitRepo, (oid_idx, (oid, kind))| {
                        let mut oid_idx = oid_idx as MyOid;
                        //todo we can eventually stop looking up and just use enumerate, but I don't trust it yet
                        let oid_idx_lookedup =
                            oid_set.get_index_of(&(*oid, *kind)).unwrap() as MyOid;
                        if oid_idx != oid_idx_lookedup {
                            println!(
                            "WARNING: oid_idx: {oid_idx} != oid_idx_lookedup: {oid_idx_lookedup}"
                        );
                            oid_idx = oid_idx_lookedup;
                        }
                        //Duplicate object
                        if acc.contains_key(&oid_idx) || !kind.is_tree() {
                            return acc;
                        }
                        let repo: &Repository = tl.get_or(|| repo.clone().to_thread_local());
                        let obj = match repo.find_object(*oid) {
                            Ok(obj) => obj,
                            Err(_) => {
                                println!("Error fetching object {oid}");
                                return acc;
                            }
                        };
                        let tree_entry_children = obj
                            .into_tree()
                            .decode()
                            .unwrap()
                            .entries
                            .iter()
                            .filter_map(|entry| {
                                let filename_idx = filenames
                                    .get_index_of(&entry.filename.to_string())
                                    .unwrap()
                                    as FilenameIdx;
                                let child_oid: ObjectId = entry.oid.into();
                                //todo we gotta standardize all these EntryKind enums
                                let child_kind = match entry.mode.into() {
                                    EntryKind::Blob => Kind::Blob,
                                    EntryKind::Tree => Kind::Tree,
                                    _ => return None,
                                };
                                //also indexSet could be an IndexMap of oid->kind
                                let child_oid_idx = oid_set
                                    .get_index_of(&(child_oid, child_kind))
                                    .unwrap()
                                    as MyOid;
                                let kind = match entry.mode.into() {
                                    EntryKind::Blob => TreeChildKind::Blob,
                                    EntryKind::Tree => TreeChildKind::Tree,
                                    _ => return None,
                                };
                                let tree_entry = MyEntry {
                                    oid_idx: child_oid_idx,
                                    filename_idx,
                                    kind,
                                };
                                let entry_idx =
                                    entry_set.get_index_of(&tree_entry).unwrap() as EntryIdx;
                                Some(entry_idx)
                            })
                            .collect::<Vec<EntryIdx>>();
                        acc.insert(
                            oid_idx,
                            TreeChild::Tree(TreeEntry {
                                oid_idx,
                                children: tree_entry_children,
                            }),
                        );
                        acc
                    },
                )
                .reduce(AHashMap::new, |mut acc, cur| {
                    acc.extend(cur);
                    acc
                });
        Ok(oid_entries)
    }
    pub fn build_oid_set(&self, repo: &Repository) -> Result<OidSet, Box<dyn std::error::Error>> {
        let start = Instant::now();
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
        println!(
            "Built object set in {} seconds",
            start.elapsed().as_secs_f64()
        );
        Ok(oid_set)
    }
    pub fn load_caches(
        &self,
        repo: &Repository,
        shared_repo: &ThreadSafeRepository,
    ) -> (FlatGitRepo, FilenameCache, FilenameSet, OidSet, EntrySet) {
        println!("Begining building in memory tree.");
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
        let filename_cache = self.build_filename_cache(&entries, &oid_entries).unwrap();

        (oid_entries, filename_cache, filenames, oid_set, entries)
    }
    pub fn build_filename_cache(
        &self,
        entry_set: &EntrySet,
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
                if *kind != TreeChildKind::Blob {
                    return acc;
                }
                if acc.contains_key(oid_idx) {
                    let acc_entry = acc.get_mut(oid_idx).unwrap();
                    acc_entry.insert(*filename_idx);
                } else {
                    acc.insert(*oid_idx, HashSet::from([*filename_idx]));
                }
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
            start.elapsed().as_secs_f64()
        );
        // idk if this helps or if the bincode library already does it when loading from file
        for (_oid, filename_set) in filename_cache.iter_mut() {
            filename_set.shrink_to_fit();
        }
        Ok(filename_cache)
    }
    pub fn walk_repo_and_collect_stats(
        &mut self,
    ) -> Result<Vec<CommitStat>, Box<dyn std::error::Error>> {
        let safe_repo = gix::ThreadSafeRepository::open(self.repo_path.clone()).unwrap();
        let inner_repo = safe_repo.clone().to_thread_local();
        let start_time = Instant::now();
        let (flat_tree, filename_cache, filename_set, oid_set, entry_set) =
            self.load_caches(&inner_repo, &safe_repo);
        println!(
            "Loaded speed-up datastructures caches in {} seconds",
            start_time.elapsed().as_secs_f64()
        );
        let mut cache = DashMap::with_capacity(flat_tree.len());
        self.batch_process_objects(
            &safe_repo,
            &mut cache,
            &filename_cache,
            &filename_set,
            &oid_set,
        );
        let tree_processing_start = Instant::now();
        let commit_count = count_commits(&inner_repo)?; //todo we could have the oid_set also store commits... idk
        println!("Found {commit_count} commits to walk through.");
        let revwalk = inner_repo
            .rev_walk(inner_repo.head_id())
            .first_parent_only()
            .use_commit_graph(true)
            .all()?;

        println!("Getting commit oids and their associated tree oids");
        let commit_oids = revwalk
            .progress_count(commit_count as u64)
            .filter_map(|info_res| match info_res {
                Ok(info) => Some((info.id, info.object().unwrap().tree().unwrap().id)),
                Err(e) => {
                    println!("Error with commit: {:?}", e);
                    None
                }
            })
            .collect::<Vec<(ObjectId, ObjectId)>>();
        println!("Collecting file results into trees for each commit:");

        let style = ProgressStyle::default_bar()
            .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {per_sec}")
            .expect("error with progress bar style");
        let commit_stats = commit_oids
            .into_par_iter()
            .rev() //todo not sure this does anything
            .progress_with_style(style)
            .map(|(commit_oid, tree_oid)| {
                let tree_oid_idx = oid_set.get_index_of(&(tree_oid, Kind::Tree)).unwrap() as MyOid;
                let tree_entry = flat_tree.get(&tree_oid_idx).unwrap().unwrap_tree();
                let (res, _processed) = self
                    .measure_tree(
                        SmallVec::new(),
                        tree_entry,
                        &cache,
                        &safe_repo,
                        &flat_tree,
                        &filename_set,
                        &oid_set,
                        &entry_set,
                    )
                    .unwrap();
                CommitStat {
                    oid: commit_oid,
                    stats: Box::new(res),
                }
            })
            .collect::<Vec<CommitStat>>();
        let elapsed_secs = tree_processing_start.elapsed().as_secs_f64();
        println!("processed: {commit_count} commits in {elapsed_secs} seconds");
        println!("{:?}", commit_stats.iter().take(5).collect::<Vec<_>>());
        Ok(commit_stats)
    }
    fn batch_process_objects(
        &mut self,
        shared_repo: &ThreadSafeRepository,
        cache: &mut ResultCache<T, F>,
        filename_cache: &FilenameCache,
        filename_set: &FilenameSet,
        oid_set: &OidSet,
    ) {
        println!("Processing all blobs in the repo");
        let start_time = Instant::now();

        //todo make this a logger info
        // let filename_cache_entries: usize = filename_cache
        //     .iter()
        //     .map(|(_oid, filenames)| filenames.iter().count())
        //     .sum();
        // println!(
        //     "Have {} objects, and {} entries in the filename cache for {} blobs",
        //     oid_set.len(),
        //     filename_cache_entries,
        //     filename_cache.len()
        // );
        let style = ProgressStyle::default_bar().template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {per_sec}").expect("error with progress bar style");
        let progress = ProgressBar::new(oid_set.len() as u64);
        progress.set_style(style);
        let tl = ThreadLocal::new();
        *cache = oid_set
            .iter()
            .enumerate()
            .par_bridge()
            .progress_with(progress)
            .fold(
                AHashMap::new,
                |mut acc: AHashMap<(MyOid, Option<FilenameIdx>), Either<T, F>>,
                 (oid_idx, (oid, kind))| {
                    if !kind.is_blob() {
                        return acc;
                    }
                    let repo: &Repository = tl.get_or(|| shared_repo.clone().to_thread_local());
                    let oid_idx = oid_idx as MyOid;
                    let Some(parent_trees) = filename_cache.get(&oid_idx) else {
                        // println!(
                        //     "No parent trees in filename cache for blob: {} with idx {}",
                        //     oid, oid_idx
                        // );
                        return acc;
                    };
                    for filename_idx in parent_trees {
                        let parent_filename =
                            filename_set.get_index(*filename_idx as usize).unwrap();
                        if let Ok(measurement) =
                            self.file_measurer
                                .measure_entry(repo, &parent_filename, oid)
                        {
                            acc.insert((oid_idx, Some(*filename_idx)), Either::Right(measurement));
                        }
                    }
                    acc
                },
            )
            .reduce(
                AHashMap::new,
                |mut acc: AHashMap<(MyOid, Option<FilenameIdx>), Either<T, F>>, cur| {
                    acc.extend(cur);
                    acc
                },
            )
            .into_iter()
            .collect();

        println!(
            "Processed {} blobs (files) in {} seconds",
            cache.len(),
            start_time.elapsed().as_secs_f64()
        );
    }
    fn measure_tree(
        &self,
        path: SmallVec<[FilenameIdx; 20]>,
        tree: &TreeEntry,
        cache: &DashMap<(MyOid, Option<FilenameIdx>), Either<T, F>>,
        repo: &ThreadSafeRepository,
        flat_tree: &FlatGitRepo,
        filename_set: &IndexSet<String>,
        oid_set: &OidSet,
        entry_set: &IndexSet<MyEntry>,
    ) -> Result<(T, usize), Box<dyn std::error::Error>> {
        if cache.contains_key(&(tree.oid_idx, None)) {
            return Ok((
                cache
                    .get(&(tree.oid_idx, None))
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
                let MyEntry {
                    oid_idx,
                    filename_idx,
                    kind,
                } = entry_set
                    .get_index(*entry_idx as usize)
                    .expect("Did not find entry_idx in entry_set");
                let mut new_path = path.clone();
                new_path.push(*filename_idx);
                let path_pieces = new_path
                    .iter()
                    .map(|idx| filename_set.get_index(*idx as usize).unwrap())
                    .map(AsRef::as_ref)
                    .collect::<Vec<&str>>();
                let entry_path = path_pieces.join("/");

                match kind {
                    TreeChildKind::Blob => {
                        let cache_key_if_blob = &(*oid_idx, Some(*filename_idx));
                        return Some((
                            entry_path,
                            cache
                                .get(cache_key_if_blob)
                                .expect("Did not find result in blob cache")
                                .clone(),
                        ));
                    }
                    TreeChildKind::Tree => {
                        let cache_key_if_folder = &(*oid_idx, None);
                        if cache.contains_key(cache_key_if_folder) {
                            return Some((
                                entry_path,
                                cache
                                    .get(cache_key_if_folder)
                                    .expect(
                                "Didn't find result in cache (folder key) even though it exists"
                            )
                                    .clone(),
                            ));
                        }
                        let child = flat_tree
                            .get(oid_idx)
                            .expect("Did not find oid_idx in flat repo");
                        let (child_result, processed) = self
                            .measure_tree(
                                new_path,
                                child.unwrap_tree(),
                                cache,
                                repo,
                                flat_tree,
                                filename_set,
                                oid_set,
                                entry_set,
                            )
                            .expect("Measure tree for oid_idx failed");
                        acc += processed;
                        match child_result.is_empty() {
                            true => None,
                            false => Some((entry_path, Either::Left(child_result))),
                        }
                    }
                }
            })
            .collect::<TreeDataCollection<T, F>>();
        let res = self.tree_reducer.reduce(repo, child_results)?;
        cache.insert((tree.oid_idx, None), Either::Left(res.clone()));
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
            start_time.elapsed().as_secs_f64()
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
