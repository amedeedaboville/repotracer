use chrono::{DateTime, Utc};
use crossbeam::channel::{self, Sender};

use dashmap::DashMap;

use globset::{Glob, GlobMatcher};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

use super::list_in_range::Granularity;
use super::repo_cache_data::{
    aliased_path_to_string, AliasedPath, FilepathIdx, MyEntry, OidIdx, RepoCacheData,
    TreeChildKind, TreeEntry,
};
use crate::collectors::list_in_range::list_commits_with_granularity;
use crate::collectors::repo_cache_data::EntryIdx;
use crate::stats::common::{FileData, FileMeasurement, PossiblyEmpty, TreeDataCollection};
use crate::util::{gix_time_to_chrono, pb_default, pb_style};
use ahash::{AHashMap, HashMap, HashSet, HashSetExt};
use gix::objs::Kind;
use gix::{Commit, ObjectId, Repository};

use indicatif::{ParallelProgressIterator, ProgressIterator};
use rayon::iter::{Either, ParallelIterator};
use rayon::prelude::*;

use smallvec::SmallVec;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Instant;
use thread_local::ThreadLocal;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitData {
    // #[serde(rename = "commit")]
    pub oid: ObjectId,
    // #[serde(
    //     serialize_with = "serialize_datetime",
    //     deserialize_with = "deserialize_datetime"
    // )]
    pub date: DateTime<Utc>,
    pub data: HashMap<String, String>,
}

type ResultCache<F> = DashMap<(OidIdx, FilepathIdx), F>;
pub struct CachedWalker<F>
where
    F: FileData + PossiblyEmpty,
{
    repo_caches: RepoCacheData,
    file_measurer: Arc<dyn FileMeasurement<Data = F> + Send>,
}
impl<F> CachedWalker<F>
where
    F: FileData + Debug + Clone + Send + Sync + 'static + PossiblyEmpty,
{
    pub fn new(
        repo_path: String,
        file_measurer: Arc<dyn FileMeasurement<Data = F> + Send>, // Changed type here
    ) -> Self {
        CachedWalker::<F> {
            repo_caches: RepoCacheData::new(&repo_path),
            file_measurer,
        }
    }
    fn gather_objects_to_process(
        &self,
        commits_to_process: &Vec<Commit>,
        path_glob: Option<GlobMatcher>,
    ) -> Result<Vec<(AliasedPath, OidIdx)>, Box<dyn std::error::Error>> {
        let RepoCacheData {
            oid_set,
            tree_entry_set,
            flat_tree,
            filename_set,
            ..
        } = &self.repo_caches;
        let mut res: HashSet<(AliasedPath, EntryIdx)> =
            HashSet::with_capacity(oid_set.num_blobs / 3);
        let mut processed: HashSet<(Option<AliasedPath>, EntryIdx)> = HashSet::new();
        let mut entries_to_process = Vec::new();
        let commit_tree_ids = commits_to_process
            .iter()
            .filter_map(|commit| commit.tree_id().map(|x| x.detach()).ok())
            .collect::<Vec<_>>();
        for commit_tree_objectid in commit_tree_ids
            .iter()
            .progress_with(pb_default(commits_to_process.len()))
        {
            let commit_tree_oid_idx = oid_set
                .get_index_of(&(*commit_tree_objectid, Kind::Tree))
                .unwrap() as OidIdx;
            let commit_tree_item = flat_tree.get(&commit_tree_oid_idx).unwrap().unwrap_tree();
            entries_to_process.extend(commit_tree_item.children.iter().map(|entry| (None, entry)));
            while let Some((maybe_path, &entry_idx)) = entries_to_process.pop() {
                let inserted = processed.insert((maybe_path.clone(), entry_idx));
                if !inserted {
                    continue;
                }
                // if maybe_path.is_some() && we have some path_in_repo function
                // then we can check it here to early return from the subtree
                let MyEntry {
                    oid_idx,
                    kind,
                    filename_idx,
                } = tree_entry_set.get_index(entry_idx as usize).unwrap();
                let mut full_path = maybe_path.clone().unwrap_or_else(SmallVec::new);
                full_path.push(*filename_idx);
                match kind {
                    TreeChildKind::Blob => {
                        if path_glob.as_ref().map_or(true, |glob| {
                            glob.is_match(aliased_path_to_string(filename_set, &full_path))
                        }) {
                            res.insert((full_path, entry_idx));
                        }
                    }
                    TreeChildKind::Tree => {
                        let child_tree = flat_tree.get(oid_idx).unwrap().unwrap_tree();
                        entries_to_process.extend(
                            child_tree
                                .children
                                .iter()
                                .map(|child_entry_idx| (Some(full_path.clone()), child_entry_idx)),
                        );
                    }
                }
            }
        }
        let mut res_vec = res.into_iter().collect::<Vec<(AliasedPath, EntryIdx)>>();
        res_vec
            .sort_by_key(|(_path, idx)| tree_entry_set.get_index(*idx as usize).unwrap().oid_idx);
        Ok(res_vec)
    }

    pub fn walk_repo_and_collect_stats(
        &mut self,
        granularity: Granularity,
        range: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
        path_in_repo: Option<String>,
    ) -> Result<Vec<CommitData>, Box<dyn std::error::Error>> {
        let RepoCacheData {
            oid_set,
            flat_tree,
            repo_safe: safe_repo,
            ..
        } = &self.repo_caches;
        let inner_repo = safe_repo.clone().to_thread_local();
        let mut cache: ResultCache<TreeDataCollection<F>> = DashMap::with_capacity(flat_tree.len());
        let commits_to_process =
            list_commits_with_granularity(&inner_repo, granularity, range.0, range.1)?;
        let num_commits = commits_to_process.len();
        println!(
            "Navigating {num_commits} commits from {:?} to {:?}",
            range.0, range.1,
        );
        let path_glob = path_in_repo.map(|path| {
            Glob::new(&path)
                .expect("Failed to compile path_in_repo into glob")
                .compile_matcher()
        });
        let entries_to_process = self.gather_objects_to_process(&commits_to_process, path_glob)?;
        self.batch_process_objects(&mut cache, entries_to_process);
        let tree_processing_start = Instant::now();
        println!("Collecting file results into trees for each commit:");
        let commit_count = num_commits;

        let oids =
            commits_to_process
                .iter()
                .map(|commit| {
                    let commit_oid = commit.id;
                    let tree_oid = commit.tree().unwrap().id;
                    (commit_oid, tree_oid)
                })
                .collect::<Vec<_>>();
        let tl = ThreadLocal::new();
        let commit_stats =
            oids.into_par_iter()
                .rev() //todo not sure this does anything
                .progress_with_style(pb_style())
                .map(|(commit_oid, tree_oid)| {
                    let tree_oid_idx =
                        oid_set.get_index_of(&(tree_oid, Kind::Tree)).unwrap() as OidIdx;
                    let tree_entry = flat_tree.get(&tree_oid_idx).unwrap().unwrap_tree();
                    let repo = tl.get_or(|| safe_repo.clone().to_thread_local());
                    let commit = repo
                        .find_object(commit_oid)
                        .expect("Could not find commit in the repo")
                        .into_commit();
                    let res = self.measure_tree_iterative(tree_entry, &cache).unwrap();
                    let data = self.file_measurer.summarize_tree_data(res).unwrap();
                    CommitData {
                        oid: commit_oid,
                        date: gix_time_to_chrono(commit.time().unwrap()),
                        data,
                    }
                })
                .collect::<Vec<CommitData>>();
        let elapsed_secs = tree_processing_start.elapsed().as_secs_f64();
        println!("processed: {commit_count} commits in {elapsed_secs} seconds");
        Ok(commit_stats)
    }

    // pub fn run_streaming_then_collect(
    //     &mut self,
    //     granularity: Granularity,
    //     range: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
    //     path_in_repo: Option<String>,
    // ) -> Result<Vec<CommitData>, Box<dyn std::error::Error>> {
    //     let (sender, receiver) = channel::unbounded();
    //     self.run_measurement_streaming(granularity, range, path_in_repo, sender)?;
    //     let commit_stats = receiver
    //         .into_iter()
    //         .map(|msg| serde_json::from_str(&msg).unwrap())
    //         .collect();
    //     Ok(commit_stats)
    // }
    fn batch_process_objects(
        &self,
        cache: &mut ResultCache<TreeDataCollection<F>>,
        entries_to_process: Vec<(AliasedPath, EntryIdx)>,
    ) {
        let start_time = Instant::now();
        println!("Processing blobs");
        let RepoCacheData {
            filename_set,
            filepath_set,
            oid_set,
            repo_safe: shared_repo,
            tree_entry_set,
            ..
        } = &self.repo_caches;
        let progress = pb_default(entries_to_process.len());
        let tl = ThreadLocal::new();
        *cache =
            entries_to_process
                .into_iter()
                .par_bridge()
                .progress_with(progress)
                .fold(
                    AHashMap::new,
                    |mut acc: AHashMap<(OidIdx, FilepathIdx), TreeDataCollection<F>>,
                     (path, entry_idx)| {
                        let path_str = aliased_path_to_string(filename_set, &path);
                        let repo: &Repository = tl.get_or(|| shared_repo.clone().to_thread_local());
                        let MyEntry { oid_idx, .. } =
                            *tree_entry_set.get_index(entry_idx as usize).unwrap();
                        let (oid, _kind) = oid_set.get_index(oid_idx).unwrap();
                        match self.file_measurer.measure_entry(repo, &path_str, oid) {
                            Ok(measurement) => {
                                let mut tree_collection: TreeDataCollection<F> = BTreeMap::new();
                                let path_idx = filepath_set
                                    .get_index_of(&path)
                                    .expect(&format!("Did not find {:?} in filepath set", path))
                                    as FilepathIdx;
                                tree_collection.insert(Either::Left(path_idx), measurement);
                                acc.insert((oid_idx, path_idx), tree_collection);
                            }
                            Err(_) => {}
                        };
                        acc
                    },
                )
                .reduce(
                    AHashMap::new,
                    |mut acc: AHashMap<(OidIdx, FilepathIdx), TreeDataCollection<F>>, cur| {
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
        cache: &ResultCache<TreeDataCollection<F>>,
    ) -> Result<TreeDataCollection<F>, Box<dyn std::error::Error>> {
        let RepoCacheData {
            flat_tree,
            tree_entry_set: entry_set,
            filename_set,
            filepath_set,
            ..
        } = &self.repo_caches;
        let empty_path_idx = filepath_set.get_index_of(&SmallVec::new()).unwrap() as FilepathIdx;
        let mut stack: VecDeque<(FilepathIdx, &TreeEntry, bool)> = VecDeque::new();
        stack.push_back((empty_path_idx, root, false));

        while let Some((path_idx, tree, do_aggregation)) = stack.pop_back() {
            if cache.contains_key(&(tree.oid_idx, path_idx)) {
                continue;
            }
            if do_aggregation {
                // let cache_key_if_folder = &(tree.oid_idx, None);
                // if cache.contains_key(cache_key_if_folder) {
                //     continue;
                // }
                let parent_path = filepath_set.get_index(path_idx as usize).unwrap();
                let tree_agg = tree
                    .children
                    .iter()
                    .filter_map(|child_idx| {
                        let MyEntry {
                            oid_idx,
                            filename_idx,
                            kind: _,
                        } = entry_set.get_index(*child_idx as usize).unwrap();
                        let mut child_path = parent_path.clone();
                        child_path.push(*filename_idx);
                        let child_path_idx =
                            filepath_set.get_index_of(&child_path).unwrap() as FilepathIdx;
                        let child_result: TreeDataCollection<F> =
                            cache.get(&(*oid_idx, child_path_idx))?.clone();
                        Some((filename_idx, child_result))
                    })
                    // Prepend the current filename to each result
                    .flat_map(|(_filename_idx, data)| {
                        data.into_iter().map(|(key, value)| {
                            let new_path_idx = match key {
                                Either::Left(_path_idx) => key,
                                Either::Right(leaf_filename_idx) => {
                                    let mut new_path = parent_path.clone();
                                    new_path.push(leaf_filename_idx);
                                    let new_path_idx = filepath_set.get_index_of(&new_path).expect(
                                        &format!(
                                            "Did not find new path in filepath set: {:?} ({})",
                                            new_path,
                                            aliased_path_to_string(filename_set, &new_path)
                                        ),
                                    )
                                        as FilepathIdx;
                                    Either::Left(new_path_idx)
                                }
                            };
                            (new_path_idx, value)
                        })
                    })
                    .collect::<TreeDataCollection<F>>();
                cache.insert((tree.oid_idx, path_idx), tree_agg);
            } else {
                stack.push_back((path_idx, tree, true));
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
                    let parent_path = filepath_set.get_index(path_idx as usize).unwrap();
                    let mut child_path = parent_path.clone();
                    child_path.push(entry.filename_idx);
                    let child_path_idx =
                        filepath_set.get_index_of(&child_path).unwrap() as FilepathIdx;
                    if !cache.contains_key(&(child_entry_idx, child_path_idx)) {
                        let Some(child) = flat_tree.get(&entry.oid_idx) else {
                            panic!("Did not find {} in flat repo", entry.oid_idx)
                        };
                        stack.push_back((child_path_idx, child.unwrap_tree(), false));
                    }
                }
            }
        }

        cache
            .get(&(root.oid_idx, empty_path_idx))
            .map(|x| x.clone())
            .ok_or_else(|| "Failed to aggregate results".into())
    }
    // fn measure_tree(
    //     &self,
    //     path: SmallVec<[FilenameIdx; 20]>,
    //     tree: &TreeEntry,
    //     cache: &ResultCache<TreeDataCollection<F>>,
    // ) -> Result<TreeDataCollection<F>, Box<dyn std::error::Error>> {
    //     let RepoCacheData {
    //         repo_safe: _repo,
    //         flat_tree,
    //         filename_set,
    //         tree_entry_set: entry_set,
    //         ..
    //     } = &self.repo_caches;
    //     if cache.contains_key(&(tree.oid_idx, None)) {
    //         return Ok(cache.get(&(tree.oid_idx, None)).unwrap().clone());
    //     }
    //     let child_results = tree
    //         .children
    //         .iter()
    //         .filter_map(|entry_idx| {
    //             let Some(entry) = entry_set.get_index(*entry_idx as usize) else {
    //                 panic!(
    //                     "Did not find {} in entry_set, even though it has {} items",
    //                     *entry_idx,
    //                     entry_set.len()
    //                 );
    //             };
    //             let MyEntry {
    //                 oid_idx,
    //                 filename_idx,
    //                 kind,
    //             } = entry;
    //             let mut new_path = path.clone();
    //             new_path.push(*filename_idx);
    //             let path_pieces = new_path
    //                 .iter()
    //                 .map(|idx| filename_set.get_index(*idx as usize).unwrap())
    //                 .map(AsRef::as_ref)
    //                 .collect::<Vec<&str>>();
    //             let entry_path = path_pieces.join("/");

    //             match kind {
    //                 TreeChildKind::Blob => {
    //                     let cache_key_if_blob = &(*oid_idx, Some(*filename_idx));
    //                     let Some(cache_res) = cache.get(cache_key_if_blob) else {
    //                         return None;
    //                         // panic!(
    //                         //     "Did not find result for {oid_idx}, {filename_idx} in blob cache\n"
    //                         // )
    //                     };
    //                     Some((entry_path, cache_res.clone()))
    //                 }
    //                 TreeChildKind::Tree => {
    //                     let cache_key_if_folder = &(*oid_idx, None);
    //                     if cache.contains_key(cache_key_if_folder) {
    //                         return Some((
    //                             entry_path,
    //                             cache
    //                                 .get(cache_key_if_folder)
    //                                 .expect(
    //                             "Didn't find result in cache (folder key) even though it exists"
    //                         )
    //                                 .clone(),
    //                         ));
    //                     }
    //                     let child = flat_tree
    //                         .get(oid_idx)
    //                         .expect("Did not find oid_idx in flat repo");
    //                     let child_result = self
    //                         .measure_tree(new_path, child.unwrap_tree(), cache)
    //                         .expect("Measure tree for oid_idx failed");
    //                     match child_result.is_empty() {
    //                         true => None,
    //                         false => Some((entry_path, child_result)),
    //                     }
    //                 }
    //             }
    //         })
    //         .flat_map(|(_path, data)| data.into_iter())
    //         .collect::<TreeDataCollection<F>>();
    //     cache.insert((tree.oid_idx, None), child_results.clone());
    //     Ok(child_results)
    // }
}

/*
#[cfg(test)]
mod tests {
    use crate::stats::tokei::{TokeiCollector, TokeiStat};

    use super::*;

    // Mock or create necessary dependencies
    // For example, a simplified RepoCacheData and FileMeasurement implementation
    // or using a mocking library to create mock objects.

    fn diff_dataframes(
        df1: &DataFrame,
        df2: &DataFrame,
    ) -> Result<DataFrame, Box<dyn std::error::Error>> {
        // Assuming df1 and df2 have the same schema and an "id" column to join on

        // Outer join on the "id" column
        let df_joined = df1.join(
            df2,
            ["commit_hash"],
            ["commit_hash"],
            JoinArgs {
                how: JoinType::Inner,
                join_nulls: false,
                slice: None,
                validation: JoinValidation::OneToOne,
                suffix: None,
            },
        )?;

        // Construct a mask to identify rows that differ
        let mask_series = Series::full_null("mask", df_joined.height(), &DataType::Boolean);
        let mut mask = mask_series.bool()?.to_owned();

        for name in df1.get_column_names() {
            if name == "id" {
                continue; // Skip the id column
            }

            // Construct column names for df1 and df2
            let col_name_1 = name.to_string();
            let col_name_2 = name.to_string();

            // Update the mask for any differences found in the current column
            let col_diff = df_joined
                .column(&col_name_1)?
                .equal_missing(df_joined.column(&col_name_2)?)?;
            mask = &mask | &col_diff;
        }

        // Invert the mask to filter out identical rows
        let mask = !mask;

        // Apply the mask to filter the DataFrame, keeping only differing rows
        Ok(df_joined.filter(&mask)?)
    }

    #[test]
    fn test_measure_tree_equivalence() {
        // Setup test environment
        let repo_path = String::from("/Users/amedee/repos/github.com/phenomnomnominal/betterer");
        let file_measurer = Box::new(TokeiCollector::new(None, None));
        let mut walker: CachedWalker<TokeiStat> =
            CachedWalker::new(repo_path.to_owned(), file_measurer);
        let recursive_res = walker
            .walk_repo_and_collect_stats(Granularity::Infinite, (None, None), None)
            .unwrap();
        let file_measurer = Box::new(TokeiCollector::new(None, None));
        walker = CachedWalker::new(repo_path.to_owned(), file_measurer);
        let iterative_res = walker
            .walk_repo_and_collect_stats(Granularity::Infinite, (None, None), None)
            .unwrap();
        // Compare their outputs
        let diff = diff_dataframes(&iterative_res, &recursive_res).unwrap();
        println!("{:?}", diff);
        assert_eq!(
            iterative_res, recursive_res,
            "The outputs of measure_tree_iterative and measure_tree should be equivalent"
        );
    }

    // Additional test cases here
}

*/
