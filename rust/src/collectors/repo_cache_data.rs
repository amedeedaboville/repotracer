use gix::objs::tree::{EntryKind, EntryMode};
use gix::objs::Kind;
use indexmap::IndexSet;
use indicatif::{ParallelProgressIterator, ProgressIterator};
use rayon::iter::{IntoParallelRefIterator, ParallelBridge, ParallelIterator};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use std::collections::{HashSet, VecDeque};
use std::fs::File;
use std::hash::Hash;
use std::io::BufWriter;
use std::io::{self, BufReader};
use std::sync::RwLock;
use thread_local::ThreadLocal;

use ahash::AHashMap;
use gix::{Commit, ObjectId, Repository, ThreadSafeRepository};
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Instant;

use std::fmt::Debug;

use crate::util::pb_style;

pub type OidIdx = u32;
pub type FlatGitRepo = AHashMap<OidIdx, TreeChild>;
//Holds unique file/folder names in the repo. In other places, instead of
//storing full filenames we store the index of the filename in this set,
//with a FilenameIdx.
pub type FilenameSet = IndexSet<String>;
pub type FilenameIdx = u32;
pub type FilepathIdx = u32;
//For each Oid, holds the list of filenames that this Oid has ever
//been referred to from.
pub type FilenameCache = AHashMap<OidIdx, HashSet<FilenameIdx>>;

pub type OidSet = IndexSet<(ObjectId, Kind)>;
pub type EntrySet = IndexSet<MyEntry>;
pub type EntryIdx = u32;

pub type AliasedPath = SmallVec<[FilenameIdx; 20]>;
pub type FilepathSet = IndexSet<AliasedPath>;
pub type PathEntrySet = IndexSet<AliasedEntry>;
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct MyEntry {
    pub oid_idx: OidIdx,
    pub filename_idx: FilenameIdx,
    pub kind: TreeChildKind,
}
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub struct AliasedEntry {
    pub oid_idx: OidIdx,
    pub filepath_idx: FilepathIdx,
    pub kind: TreeChildKind,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub enum TreeChildKind {
    Blob,
    Tree,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TreeEntry {
    pub oid_idx: OidIdx,
    pub children: Vec<EntryIdx>,
}
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BlobParent {
    pub tree_id: usize,
    pub filename_idx: usize,
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OidSetWithInfo {
    set: OidSet,
    pub num_trees: usize,
    pub num_blobs: usize,
}
impl OidSetWithInfo {
    pub fn iter_trees(&self) -> impl Iterator<Item = &(ObjectId, Kind)> + '_ {
        self.set.iter().filter(|(_, kind)| kind.is_tree())
    }
    pub fn iter_blobs(&self) -> impl Iterator<Item = &(ObjectId, Kind)> + '_ {
        self.set.iter().filter(|(_, kind)| kind.is_blob())
    }
    pub fn get_index_of(&self, elem: &(ObjectId, Kind)) -> Option<usize> {
        self.set.get_index_of(elem)
    }
    pub fn get_index(&self, idx: OidIdx) -> Option<&(ObjectId, Kind)> {
        self.set.get_index(idx as usize)
    }
    pub fn insert_full(&mut self, obj: (ObjectId, Kind)) -> (usize, bool) {
        self.set.insert_full(obj)
    }
}
pub struct RepoCacheData {
    pub repo_safe: ThreadSafeRepository,
    pub oid_set: OidSetWithInfo,
    pub filename_set: FilenameSet,
    pub filepath_set: FilepathSet,
    pub flat_tree: FlatGitRepo,
    pub filename_cache: FilenameCache,
    pub tree_entry_set: EntrySet,
    pub path_entry_set: PathEntrySet,
}

impl RepoCacheData {
    pub fn new(repo_path: &str) -> Self {
        // let start_time = Instant::now();
        let repo_safe = ThreadSafeRepository::open(repo_path).unwrap();
        let repo = repo_safe.clone().to_thread_local();
        let (flat_tree, filename_cache, filename_set, filepath_set, oid_set, path_entry_set) =
            load_caches(&repo, &repo_safe);
        RepoCacheData {
            repo_safe,
            oid_set,
            filename_set,
            filepath_set,
            flat_tree,
            path_entry_set,
            filename_cache,
            tree_entry_set: IndexSet::new(),
        }
        // println!(
        //     "Loaded repo caches in {} seconds",
        //     start_time.elapsed().as_secs_f64()
        // );
    }
}
pub fn build_oid_set(repo: &Repository) -> Result<OidSetWithInfo, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let mut num_trees = 0;
    let mut num_blobs = 0;
    let mut oids = Vec::new();
    for oid_res in repo
        .objects
        .iter()
        .unwrap()
        .with_ordering(gix::odb::store::iter::Ordering::PackAscendingOffsetThenLooseLexicographical)
    {
        if let Ok(oid) = oid_res {
            let kind = repo.find_header(oid).unwrap().kind();
            match kind {
                Kind::Tree => num_trees += 1,
                Kind::Blob => num_blobs += 1,
                _ => continue,
            }
            oids.push((oid, kind));
        }
    }
    let oid_set = oids.into_iter().collect::<IndexSet<(ObjectId, Kind)>>();
    println!(
        "Built object set in {} seconds",
        start.elapsed().as_secs_f64()
    );
    Ok(OidSetWithInfo {
        set: oid_set,
        num_trees,
        num_blobs,
    })
}
pub fn build_filename_set(
    repo: &ThreadSafeRepository,
    oid_set: &OidSetWithInfo,
) -> Result<FilenameSet, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let tl = ThreadLocal::new();
    let style = ProgressStyle::default_bar().template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {per_sec}").expect("error with progress bar style");
    let progress = ProgressBar::new(oid_set.num_trees as u64);
    progress.set_style(style);
    let filenames: FilenameSet = oid_set
        .iter_trees()
        .progress_with(progress)
        .par_bridge()
        .fold(IndexSet::new, |mut acc: IndexSet<String>, (oid, _kind)| {
            let repo: &Repository = tl.get_or(|| repo.clone().to_thread_local());
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
    repo: &ThreadSafeRepository,
    oid_set: &OidSetWithInfo,
    filename_set: &FilenameSet,
) -> Result<EntrySet, Box<dyn std::error::Error>> {
    let tl = ThreadLocal::new();
    let progress = ProgressBar::new(oid_set.num_trees as u64);
    progress.set_style(pb_style());
    let entries = oid_set
        .iter_trees()
        .par_bridge()
        .progress_with(progress)
        .fold(IndexSet::new, |mut acc: EntrySet, (oid, _kind)| {
            let repo: &Repository = tl.get_or(|| repo.clone().to_thread_local());
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
                let other_kind = match entry.mode.into() {
                    EntryKind::Blob => Kind::Blob,
                    EntryKind::Tree => Kind::Tree,
                    _ => {
                        println!("unreachable new code");
                        continue;
                    }
                };
                let child_oid_idx =
                    oid_set.get_index_of(&(child_oid, other_kind)).unwrap() as OidIdx;
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

pub fn build_caches_with_paths(
    repo: &Repository,
    oid_set: &OidSetWithInfo,
) -> Result<(FilenameSet, FilepathSet, PathEntrySet, FlatGitRepo), Box<dyn std::error::Error>> {
    let mut filename_set: FilenameSet = IndexSet::new();
    let mut filepath_set: FilepathSet = IndexSet::new();
    let mut path_entry_set: PathEntrySet = IndexSet::new();
    let mut flat_tree: FlatGitRepo = AHashMap::new();
    let mut stack: VecDeque<(SmallVec<[FilenameIdx; 20]>, ObjectId)> = VecDeque::new();

    let locked_filename_set = RwLock::new(filename_set);
    let locked_filepath_set = RwLock::new(filepath_set);
    let locked_path_entry_set = RwLock::new(path_entry_set);

    let start_time = Instant::now();
    println!("Adding commits to visit to stack.");
    let revwalk = repo
        .rev_walk(repo.head_id())
        .first_parent_only()
        .use_commit_graph(true)
        .all()?;
    for info_result in revwalk {
        let info = info_result?;
        let commit = info.object().unwrap();
        let tree = commit.tree().unwrap();
        stack.push_back((SmallVec::from_slice(&[]), tree.id().into()));
    }
    println!(
        "Added commits {} seconds.",
        start_time.elapsed().as_secs_f32()
    );

    let start_time = Instant::now();
    println!("Have {} trees in the stack. Starting the DFS", stack.len());
    let progress = ProgressBar::new(oid_set.num_trees as u64);
    progress.set_style(pb_style());
    while let Some((path, tree_oid)) = stack.pop_back() {
        progress.inc(1);
        let tree = repo
            .find_object(tree_oid)
            .expect("Could not find commit in the repo")
            .into_tree();
        let obj = tree.decode().unwrap();
        let obj_oid_idx = oid_set.get_index_of(&(tree_oid, Kind::Tree)).unwrap() as OidIdx;
        if flat_tree.contains_key(&obj_oid_idx) {
            continue;
        }
        let entries_and_to_push: Vec<(EntryIdx, Option<(AliasedPath, ObjectId)>)> = obj
            .entries
            .par_iter()
            .filter_map(|entry| {
                let kind = match entry.mode.into() {
                    EntryKind::Blob => Kind::Blob,
                    EntryKind::Tree => Kind::Tree,
                    _ => return None,
                };
                let entry_oid: ObjectId = entry.oid.into();
                let oid_idx = oid_set.get_index_of(&(entry_oid, kind)).unwrap() as OidIdx;
                let (filename_idx, _) = {
                    let mut filename_set = locked_filename_set.write().unwrap();
                    filename_set.insert_full(entry.filename.to_string())
                };
                let mut full_path = path.clone();
                full_path.push(filename_idx as FilenameIdx);
                let (filepath_idx, _) = {
                    let mut filepath_set = locked_filepath_set.write().unwrap();
                    filepath_set.insert_full(full_path.clone())
                };
                let kind = match entry.mode.into() {
                    EntryKind::Blob => TreeChildKind::Blob,
                    EntryKind::Tree => TreeChildKind::Tree,
                    _ => return None,
                };
                let (entry_idx, _) = {
                    let mut path_entry_set = locked_path_entry_set.write().unwrap();
                    path_entry_set.insert_full(AliasedEntry {
                        oid_idx: oid_idx as OidIdx,
                        filepath_idx: filepath_idx as FilepathIdx,
                        kind,
                    })
                };
                let to_push =
                    if entry.mode.is_tree() && !flat_tree.contains_key(&oid_idx) {
                        Some((full_path, entry_oid))
                    } else {
                        None
                    };
                Some((entry_idx as EntryIdx, to_push))
            })
            .collect();
        let children_entries = entries_and_to_push
            .iter()
            .map(|(entry_idx, _)| *entry_idx)
            .collect::<Vec<EntryIdx>>();
        for (_, to_push) in entries_and_to_push {
            if let Some((path, oid)) = to_push {
                stack.push_back((path, oid));
            }
        }
        flat_tree.insert(
            obj_oid_idx,
            TreeChild::Tree(TreeEntry {
                oid_idx: obj_oid_idx,
                children: children_entries,
            }),
        );
    }
    println!("Done with the DFS");
    println!(
        "Built flat tree in {} seconds.",
        start_time.elapsed().as_secs_f32()
    );
    Ok((
        locked_filename_set.into_inner().unwrap(),
        locked_filepath_set.into_inner().unwrap(),
        locked_path_entry_set.into_inner().unwrap(),
        flat_tree,
    ))
}
pub fn build_flat_tree(
    repo: &ThreadSafeRepository,
    oid_set: &OidSetWithInfo,
    entry_set: &EntrySet,
    filenames: &FilenameSet,
) -> Result<FlatGitRepo, Box<dyn std::error::Error>> {
    let tl = ThreadLocal::new();
    let progress = ProgressBar::new(oid_set.num_trees as u64);
    progress.set_style(pb_style());
    let oid_entries: AHashMap<OidIdx, TreeChild> = oid_set
        .iter_trees()
        .progress_with(progress)
        .par_bridge()
        .fold(AHashMap::new, |mut acc: FlatGitRepo, (oid, kind)| {
            let oid_idx = oid_set.get_index_of(&(*oid, *kind)).unwrap() as OidIdx;
            //Duplicate object
            if acc.contains_key(&oid_idx) {
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
                    let filename_idx =
                        filenames.get_index_of(&entry.filename.to_string()).unwrap() as FilenameIdx;
                    let child_oid: ObjectId = entry.oid.into();
                    //todo we gotta standardize all these EntryKind enums
                    let child_kind = match entry.mode.into() {
                        EntryKind::Blob => Kind::Blob,
                        EntryKind::Tree => Kind::Tree,
                        _ => return None,
                    };
                    //also indexSet could be an IndexMap of oid->kind
                    let child_oid_idx =
                        oid_set.get_index_of(&(child_oid, child_kind)).unwrap() as OidIdx;
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
                    let entry_idx = entry_set.get_index_of(&tree_entry).unwrap() as EntryIdx;
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
        })
        .reduce(AHashMap::new, |mut acc, cur| {
            acc.extend(cur);
            acc
        });
    Ok(oid_entries)
}
pub fn load_caches(
    repo: &Repository,
    shared_repo: &ThreadSafeRepository,
) -> (
    FlatGitRepo,
    FilenameCache,
    FilenameSet,
    FilepathSet,
    OidSetWithInfo,
    PathEntrySet,
) {
    // let repo_path = repo.path().to_str().unwrap();
    let oid_set: OidSetWithInfo = load_and_save_cache(shared_repo, "oids", |_| build_oid_set(repo));
    println!(
        "Found {} objects: {} trees and {} blobs.",
        oid_set.set.len(),
        oid_set.num_trees,
        oid_set.num_blobs
    );

    // let filenames: FilenameSet = load_and_save_cache(shared_repo, "filenames", |shared_repo| {
    //     build_filename_set(shared_repo, &oid_set)
    // });
    // let entries: EntrySet = load_and_save_cache(shared_repo, "entries", |shared_repo| {
    //     build_entries_set(shared_repo, &oid_set, &filenames)
    // });
    // let oid_entries = load_and_save_cache(shared_repo, "flat_tree", |shared_repo| {
    //     build_flat_tree(shared_repo, &oid_set, &entries, &filenames)
    // });
    let (filename_set, filepath_set, path_entry_set, flat_tree) =
        build_caches_with_paths(repo, &oid_set).unwrap();
    let filename_cache = build_filename_cache(&path_entry_set, &flat_tree).unwrap();

    (
        flat_tree,
        filename_cache,
        filename_set,
        filepath_set,
        oid_set,
        path_entry_set,
    )
}
fn load_and_save_cache<T: Serialize + DeserializeOwned>(
    repo: &ThreadSafeRepository,
    cache_name: &str,
    build_fn: impl FnOnce(&ThreadSafeRepository) -> Result<T, Box<dyn std::error::Error>>,
) -> T {
    let start_time = Instant::now();
    let repo_path = repo.path().to_str().unwrap();
    match load_cache::<T>(repo_path, cache_name) {
        Ok(f) => {
            println!(
                "Loaded {cache_name} from cache in {} seconds.",
                start_time.elapsed().as_secs_f32()
            );
            f
        }
        Err(e) => {
            println!("Computing {cache_name} from scratch due to error: {}", e);
            let cache = build_fn(repo).unwrap();
            save_cache(repo_path, cache_name, &cache).unwrap();
            println!(
                "Built {cache_name} in {} seconds",
                start_time.elapsed().as_secs_f32(),
            );
            cache
        }
    }
}
fn build_filename_cache(
    entry_set: &PathEntrySet,
    flat_repo: &FlatGitRepo,
) -> Result<FilenameCache, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let progress = ProgressBar::new(entry_set.len() as u64);
    progress.set_style(pb_style());
    let mut filename_cache = entry_set
        .iter()
        .par_bridge()
        .progress_with(progress)
        .fold(
            AHashMap::new,
            |mut acc: FilenameCache, entry: &AliasedEntry| {
                let AliasedEntry {
                    oid_idx,
                    filepath_idx,
                    kind,
                } = entry;
                if *kind != TreeChildKind::Blob {
                    return acc;
                }
                if acc.contains_key(oid_idx) {
                    let acc_entry = acc.get_mut(oid_idx).unwrap();
                    acc_entry.insert(*filepath_idx);
                } else {
                    acc.insert(*oid_idx, HashSet::from([*filepath_idx]));
                }
                acc
            },
        )
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

fn load_cache<Cache>(repo_path: &str, cache_name: &str) -> io::Result<Cache>
where
    Cache: DeserializeOwned,
{
    let start_time = Instant::now();
    let cache_path = format!("{}/{}.bin", repo_path, cache_name);
    // println!("Loading {cache_name} from {cache_path}");
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
fn save_cache<Cache>(repo_path: &str, cache_name: &str, cache: &Cache) -> io::Result<()>
where
    Cache: Serialize,
{
    let cache_path = format!("{}/{}.bin", repo_path, cache_name);
    let cache_file = File::create(cache_path)?;
    let mut cache_writer = BufWriter::new(cache_file);
    bincode::serialize_into(&mut cache_writer, cache)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    Ok(())
}
