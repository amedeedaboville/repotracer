use chrono::{DateTime, Utc};
use dashmap::DashMap;
use gix::index::Entry;
use polars::frame::row::Row;
use std::collections::{BTreeMap, VecDeque};
use std::error::Error;

use super::list_in_range::Granularity;
use super::repo_cache_data::{
    FilenameIdx, MyEntry, MyOid, RepoCacheData, TreeChildKind, TreeEntry,
};
use crate::collectors::list_in_range::list_commits_with_granularity;
use crate::collectors::repo_cache_data::EntryIdx;
use crate::polars_utils::rows_to_df;
use crate::stats::common::{FileMeasurement, PossiblyEmpty, TreeDataCollection};
use crate::util::{pb_default, pb_style};
use ahash::{AHashMap, HashSet, HashSetExt};
use gix::objs::Kind;
use gix::{Commit, ObjectId, Repository};

use indicatif::ProgressBar;
use indicatif::{ParallelProgressIterator, ProgressIterator};
use polars::prelude::*;
use rayon::iter::ParallelIterator;
use rayon::prelude::*;
use serde::Serialize;
use smallvec::SmallVec;
use std::fmt::Debug;
use std::time::Instant;
use thread_local::ThreadLocal;

enum Task {
    ProcessNode(EntryIdx),
    AggregateNode(EntryIdx),
}
#[derive(Debug)]
pub struct CommitStat<'a> {
    // #[serde(rename = "commit")]
    pub oid: ObjectId,
    pub time: gix::date::Time,
    pub stats: Arc<(Schema, Row<'a>)>,
}
unsafe impl<'a> Send for CommitStat<'a> {}
unsafe impl<'a> Sync for CommitStat<'a> {}
fn count_commits(repo: &Repository) -> Result<usize, Box<dyn std::error::Error>> {
    Ok(repo
        .rev_walk(repo.head_id())
        .first_parent_only()
        .all()?
        .count())
}

type ResultCache<F> = DashMap<(MyOid, Option<FilenameIdx>), F>;
pub struct CachedWalker<F> {
    repo_caches: RepoCacheData,
    file_measurer: Box<dyn FileMeasurement<F>>,
}
impl<FileData> CachedWalker<FileData>
where
    FileData: Debug + Clone + Send + Sync + 'static + PossiblyEmpty,
{
    pub fn new(
        repo_path: String,
        file_measurer: Box<dyn FileMeasurement<FileData>>, // Changed type here
    ) -> Self {
        CachedWalker::<FileData> {
            repo_caches: RepoCacheData::new(&repo_path),
            file_measurer,
        }
    }
    fn gather_objects_to_process(
        &self,
        commits_to_process: &Vec<Commit>,
    ) -> Result<Vec<MyOid>, Box<dyn std::error::Error>> {
        let RepoCacheData {
            oid_set,
            tree_entry_set,
            flat_tree,
            ..
        } = &self.repo_caches;
        let mut res: HashSet<EntryIdx> = HashSet::with_capacity(oid_set.num_blobs / 3);
        let processed: HashSet<EntryIdx> = HashSet::new();
        for commit in commits_to_process
            .iter()
            .progress_with(pb_default(commits_to_process.len()))
        {
            let commit_tree_objectid = commit.tree()?.id;
            let commit_tree_oid_idx = oid_set
                .get_index_of(&(commit_tree_objectid, Kind::Tree))
                .unwrap() as MyOid;
            let commit_tree_item = flat_tree.get(&commit_tree_oid_idx).unwrap().unwrap_tree();
            let mut entries_to_process = commit_tree_item.children.clone();
            while let Some(entry_idx) = entries_to_process.pop() {
                if processed.contains(&entry_idx) {
                    continue;
                }
                let MyEntry { oid_idx, kind, .. } =
                    tree_entry_set.get_index(entry_idx as usize).unwrap();
                match kind {
                    TreeChildKind::Blob => {
                        res.insert(entry_idx);
                    }
                    TreeChildKind::Tree => {
                        let child_tree = flat_tree.get(oid_idx).unwrap().unwrap_tree();
                        entries_to_process.extend(child_tree.children.clone());
                    }
                }
            }
        }
        let mut res_vec = res.into_iter().collect::<Vec<EntryIdx>>();
        res_vec.sort_by_key(|idx| tree_entry_set.get_index(*idx as usize).unwrap().oid_idx);
        Ok(res_vec)
    }

    pub fn walk_repo_and_collect_stats(
        &mut self,
        granularity: Granularity,
        range: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
    ) -> Result<DataFrame, Box<dyn std::error::Error>> {
        let RepoCacheData {
            oid_set,
            flat_tree,
            repo_safe: safe_repo,
            tree_entry_set,
            ..
        } = &self.repo_caches;
        let inner_repo = safe_repo.clone().to_thread_local();
        let mut cache: DashMap<(MyOid, Option<FilenameIdx>), TreeDataCollection<FileData>> =
            DashMap::with_capacity(flat_tree.len());
        println!("Getting commits to process");
        let commits_to_process =
            list_commits_with_granularity(&inner_repo, granularity, range.0, range.1)?;
        let num_commits = commits_to_process.len();
        println!("Getting entries to process");
        // todo add a heuristic to determine whether it's worth it to gather all entries
        // eg maybe more than 1/3rd of the total repo size we process all the entries
        let entries_to_process = self.gather_objects_to_process(&commits_to_process).ok();
        println!(
            "Processing {num_commits} commits from {:?} to {:?}",
            range.0, range.1,
        );
        self.batch_process_objects(&mut cache, entries_to_process);
        let tree_processing_start = Instant::now();
        println!("Collecting file results into trees for each commit:");
        let commit_count = num_commits;

        let tl = ThreadLocal::new();
        let oids =
            commits_to_process
                .iter()
                .map(|commit| {
                    let commit_oid = commit.id;
                    let tree_oid = commit.tree().unwrap().id;
                    (commit_oid, tree_oid)
                })
                .collect::<Vec<_>>();
        let commit_stats = oids
            .into_par_iter()
            .rev() //todo not sure this does anything
            .progress_with_style(pb_style())
            .map(|(commit_oid, tree_oid)| {
                let tree_oid_idx = oid_set.get_index_of(&(tree_oid, Kind::Tree)).unwrap() as MyOid;
                let tree_entry = flat_tree.get(&tree_oid_idx).unwrap().unwrap_tree();
                let repo = tl.get_or(|| safe_repo.clone().to_thread_local());
                let commit = repo
                    .find_object(commit_oid)
                    .expect("Could not find commit in the repo")
                    .into_commit();
                let res = self.measure_tree_iterative(&tree_entry, &cache).unwrap();
                let (schema, row) = self.file_measurer.summarize_tree_data(res).unwrap();
                CommitStat {
                    oid: commit_oid,
                    time: commit.time().unwrap(),
                    stats: Arc::new((schema, row)),
                }
            })
            .collect::<Vec<CommitStat>>();
        let elapsed_secs = tree_processing_start.elapsed().as_secs_f64();
        println!("processed: {commit_count} commits in {elapsed_secs} seconds");
        println!("{:?}", commit_stats.iter().take(5).collect::<Vec<_>>());
        let schemas = commit_stats.iter().map(|cs| cs.stats.0.clone());
        let rows = commit_stats.iter().map(|cs| &cs.stats.1);
        let mut func_df = rows_to_df(schemas, rows)?;
        println!("turned commit_stats into a DF");
        let commit_shas = commit_stats
            .iter()
            .map(|cs| cs.oid.to_string())
            .collect::<Vec<_>>();
        let commit_times = commit_stats
            .iter()
            .map(|cs| AnyValue::Datetime(cs.time.seconds * 1000, TimeUnit::Milliseconds, &None))
            .collect::<Vec<_>>();
        func_df.insert_column(0, Series::new("commit_hash", commit_shas))?;
        func_df.insert_column(1, Series::new("commit_time", commit_times))?;
        Ok(func_df)
    }
    fn batch_process_objects(
        &self,
        cache: &mut ResultCache<TreeDataCollection<FileData>>,
        entries_to_process: Option<Vec<EntryIdx>>,
    ) {
        let start_time = Instant::now();
        println!("Processing blobs");
        let RepoCacheData {
            filename_cache,
            filename_set,
            oid_set,
            repo_safe: shared_repo,
            tree_entry_set,
            ..
        } = &self.repo_caches;
        let total = if let Some(entries) = &entries_to_process {
            entries.len()
        } else {
            oid_set.num_blobs
        } as u64;
        let iter: Box<dyn Iterator<Item = (u32, u32)> + Send> = match entries_to_process {
            Some(entries) => Box::new(entries.into_iter().map(move |idx| {
                let MyEntry {
                    oid_idx,
                    filename_idx,
                    ..
                } = tree_entry_set.get_index(idx as usize).unwrap();
                (*oid_idx, *filename_idx)
            })),
            None => {
                Box::new(oid_set.iter_blobs().flat_map(|(oid, _kind)| {
                    let oid_idx = oid_set.get_index_of(&(*oid, Kind::Blob)).unwrap() as MyOid;
                    let parent_trees = filename_cache.get(&oid_idx);
                    match parent_trees {
                        None => vec![].into_iter(), // this is where we used to log
                        Some(parent_trees) => parent_trees
                            .iter()
                            .map(move |filename_idx| (oid_idx, *filename_idx))
                            .collect::<Vec<(u32, u32)>>()
                            .into_iter(), // Convert to Vec and then to an iterator
                    }
                }))
            }
        };
        let progress = ProgressBar::new(total);
        progress.set_style(pb_style());
        let tl = ThreadLocal::new();
        *cache = iter
            .par_bridge()
            .progress_with(progress)
            .fold(
                AHashMap::new,
                |mut acc: AHashMap<(MyOid, Option<FilenameIdx>), TreeDataCollection<FileData>>,
                 (oid_idx, filename_idx)| {
                    let repo: &Repository = tl.get_or(|| shared_repo.clone().to_thread_local());
                    let (oid, _kind) = oid_set.get_index(oid_idx).unwrap();
                    let parent_filename = filename_set.get_index(filename_idx as usize).unwrap();
                    match self.file_measurer.measure_entry(repo, parent_filename, oid) {
                        Ok(measurement) => {
                            let mut tree_collection: TreeDataCollection<FileData> = BTreeMap::new();
                            tree_collection.insert(parent_filename.clone(), measurement);
                            acc.insert((oid_idx, Some(filename_idx)), tree_collection);
                        }
                        Err(_) => {}
                    };
                    acc
                },
            )
            .reduce(
                AHashMap::new,
                |mut acc: AHashMap<(MyOid, Option<FilenameIdx>), TreeDataCollection<FileData>>,
                 cur| {
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

    fn measure_tree_iterative(
        &self,
        root: &TreeEntry,
        cache: &DashMap<(MyOid, Option<FilenameIdx>), TreeDataCollection<FileData>>,
    ) -> Result<TreeDataCollection<FileData>, Box<dyn std::error::Error>> {
        let RepoCacheData {
            flat_tree,
            filename_set,
            tree_entry_set: entry_set,
            ..
        } = &self.repo_caches;
        let mut stack: VecDeque<(SmallVec<[FilenameIdx; 20]>, &TreeEntry, bool)> = VecDeque::new();
        stack.push_back((SmallVec::from_slice(&[]), root, false));

        while let Some((path, tree, do_aggregation)) = stack.pop_back() {
            if cache.contains_key(&(tree.oid_idx, None)) {
                continue;
            }
            if !do_aggregation {
                stack.push_back((path.clone(), tree, true));
                for &child_entry_idx in tree.children.iter().rev() {
                    let Some(entry) = entry_set.get_index(child_entry_idx as usize) else {
                        panic!(
                            "Did not find {} in entry_set, even though it has {} items",
                            child_entry_idx,
                            entry_set.len()
                        );
                    };
                    if entry.kind == TreeChildKind::Blob {
                        continue;
                    }
                    let Some(child) = flat_tree.get(&entry.oid_idx) else {
                        panic!("Did not find {} in flat repo", entry.oid_idx)
                    };
                    match entry.kind {
                        TreeChildKind::Blob => {
                            if !cache.contains_key(&(child_entry_idx, Some(entry.filename_idx))) {
                                let mut child_path = path.clone();
                                child_path.push(entry.filename_idx);
                                stack.push_back((child_path, child.unwrap_tree(), false));
                            }
                        }
                        TreeChildKind::Tree => {
                            if !cache.contains_key(&(child_entry_idx, None)) {
                                let mut child_path = path.clone();
                                child_path.push(entry.filename_idx);
                                stack.push_back((child_path, child.unwrap_tree(), false));
                            }
                        }
                    }
                }
            } else {
                let tree_agg = tree
                    .children
                    .iter()
                    .filter_map(|child_idx| {
                        let MyEntry {
                            oid_idx,
                            filename_idx,
                            kind,
                        } = entry_set.get_index(*child_idx as usize).unwrap();
                        let child_result: TreeDataCollection<FileData> = match kind {
                            TreeChildKind::Blob => {
                                cache.get(&(*oid_idx, Some(*filename_idx)))?.clone()
                            }
                            // .unwrap_or_else(|| {
                            //     println!(
                            //         "Measure tree for child entry {oid_idx} ({:?}) failed: {:?} ({})",
                            //             self.repo_caches
                            //                 .oid_set
                            //                 .get_index(*oid_idx)
                            //                 .unwrap(),
                            //             filename_idx,
                            //             filename_set.get_index(*filename_idx as usize).unwrap()
                            //     )
                            // })
                            TreeChildKind::Tree => cache.get(&(*oid_idx, None))?.clone(),
                            /*
                                .unwrap_or_else(|| {
                                panic!(
                                    "Measure tree for tree entry {oid_idx} ({:?}) failed: {:?} ({})",
                                        self.repo_caches
                                            .oid_set
                                            .get_index(*oid_idx)
                                            .unwrap(),
                                        filename_idx,
                                        filename_set.get_index(*filename_idx as usize).unwrap()
                                )
                            })
                                .clone(),
                                */
                        };
                        let child_filename =
                            filename_set.get_index(*filename_idx as usize).unwrap();
                        Some((child_filename, child_result))
                    })
                    .flat_map(|(path, data_collection)| {
                        data_collection
                            .into_iter()
                            .map(move |(k, v)| (format!("{}/{}", path, k), v))
                    })
                    .collect::<TreeDataCollection<FileData>>();
                cache.insert((tree.oid_idx, None), tree_agg);
            }
        }

        cache
            .get(&(root.oid_idx, None))
            .map(|x| x.clone())
            .ok_or_else(|| "Failed to aggregate results".into())
    }
    fn measure_tree(
        &self,
        path: SmallVec<[FilenameIdx; 20]>,
        tree: &TreeEntry,
        cache: &DashMap<(MyOid, Option<FilenameIdx>), TreeDataCollection<FileData>>,
    ) -> Result<(TreeDataCollection<FileData>, usize), Box<dyn std::error::Error>> {
        let RepoCacheData {
            repo_safe: repo,
            flat_tree,
            filename_set,
            tree_entry_set: entry_set,
            ..
        } = &self.repo_caches;
        if cache.contains_key(&(tree.oid_idx, None)) {
            return Ok((cache.get(&(tree.oid_idx, None)).unwrap().clone(), 0));
        }
        let mut acc = 0;
        let child_results = tree
            .children
            .iter()
            .filter_map(|entry_idx| {
                let Some(entry) = entry_set.get_index(*entry_idx as usize) else {
                    panic!(
                        "Did not find {} in entry_set, even though it has {} items",
                        *entry_idx,
                        entry_set.len()
                    );
                };
                let MyEntry {
                    oid_idx,
                    filename_idx,
                    kind,
                } = entry;
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
                        let Some(cache_res) = cache.get(cache_key_if_blob) else {
                            return None;
                            // panic!(
                            //     "Did not find result for {oid_idx}, {filename_idx} in blob cache\n"
                            // )
                        };
                        return Some((entry_path, cache_res.clone()));
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
                            .measure_tree(new_path, child.unwrap_tree(), cache)
                            .expect("Measure tree for oid_idx failed");
                        acc += processed;
                        match child_result.is_empty() {
                            true => None,
                            false => Some((entry_path, child_result)),
                        }
                    }
                }
            })
            .map(|(path, data)| data.into_iter().map(|(k, v)| (k, v)))
            .flatten()
            .collect::<TreeDataCollection<FileData>>();
        // let res: TreeData = TreeData::reduce(repo, child_results)?;
        cache.insert((tree.oid_idx, None), child_results.clone());
        Ok((child_results, acc))
    }
}
