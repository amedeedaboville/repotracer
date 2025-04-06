use chrono::{DateTime, Utc};

use dashmap::DashMap;

use globset::{Glob, GlobMatcher};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
use gix::{ObjectId, Repository};

use indicatif::{ParallelProgressIterator, ProgressIterator};
use rayon::iter::{Either, ParallelIterator};
use rayon::prelude::*;

use smallvec::SmallVec;
use std::fmt::Debug;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Instant;
use thread_local::ThreadLocal;

/// Represents a git object at a specific path in a git tree
#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
struct EntryKey {
    oid_idx: OidIdx,
    path_idx: FilepathIdx,
}

impl EntryKey {
    fn new(oid_idx: OidIdx, path_idx: FilepathIdx) -> Self {
        Self { oid_idx, path_idx }
    }
}

#[derive(Debug)]
struct AggTask<F> {
    id: usize,
    entry_key: EntryKey,
    children_info: Vec<(Either<EntryKey, usize>, u32, TreeChildKind)>,
    is_commit_root: bool,
    commit_info: Option<(ObjectId, DateTime<Utc>)>,
    result: OnceLock<Option<TreeDataCollection<F>>>,
    depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitData {
    #[serde(rename = "commit")]
    pub oid: ObjectId,
    pub date: DateTime<Utc>,
    pub data: HashMap<String, String>,
}

type ResultCache<F> = DashMap<EntryKey, Option<F>>;

//todo figure out what to call this or whether to put it in the Stat options themselves
//probably should go in the stat options
pub struct MeasurementRunOptions {
    pub granularity: Granularity,
    pub range: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
    pub path_in_repo: Option<String>,
}

struct BuildTaskGraphResult<F> {
    all_tasks: Vec<AggTask<F>>,
    tasks_by_depth: Vec<Vec<usize>>,
    entries_to_process_set: HashSet<(AliasedPath, EntryIdx)>,
}

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
    pub fn walk_repo_and_collect_stats(
        &self,
        options: MeasurementRunOptions,
    ) -> Result<Vec<CommitData>, Box<dyn std::error::Error>> {
        let measurement_start = Instant::now();
        let MeasurementRunOptions {
            granularity,
            range,
            path_in_repo,
        } = options;
        let RepoCacheData {
            flat_tree,
            repo_safe: safe_repo,
            tree_entry_set,
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
        let empty_path_idx = self.repo_caches.get_empty_path_idx();
        let commits_with_info = commits_to_process
            .iter()
            .map(|commit| {
                let commit_oid = commit.id;
                let tree_oid = commit.tree().unwrap().id;
                let commit_time = commit.time().unwrap();
                (commit_oid, tree_oid, commit_time)
            })
            .collect::<Vec<_>>();

        println!("Building task graph and identifying blobs via commit traversal:");
        let task_graph_start = Instant::now();
        let BuildTaskGraphResult {
            all_tasks,
            tasks_by_depth,
            entries_to_process_set,
        } = Self::build_task_graph(
            &commits_with_info,
            &self.repo_caches,
            path_glob.as_ref(),
            empty_path_idx,
        );
        println!(
            "Created {} tasks and identified {} unique blobs in {:.2} secs",
            all_tasks.len(),
            entries_to_process_set.len(),
            task_graph_start.elapsed().as_secs_f64()
        );

        let mut entries_to_process_vec = entries_to_process_set.into_iter().collect::<Vec<_>>();
        // this is very important, we need to process the blobs in the order they exist in the git odb
        entries_to_process_vec
            .sort_by_key(|(_path, idx)| tree_entry_set.get_index(*idx as usize).unwrap().oid_idx);
        self.batch_process_objects(&mut cache, entries_to_process_vec);

        println!("Executing {} tasks level-by-level:", all_tasks.len()); // Use direct len()
        let parallel_start = Instant::now();
        let commit_results = Arc::new(DashMap::<ObjectId, CommitData>::new());
        let file_measurer = self.file_measurer.clone();
        let all_tasks_arc = Arc::new(all_tasks);
        let blob_cache = Arc::new(cache);
        let overall_progress = pb_default(all_tasks_arc.len());
        overall_progress.set_style(pb_style());

        for tasks_at_this_depth in tasks_by_depth.iter() {
            tasks_at_this_depth
                .par_iter()
                .for_each(|&task_id| {
                    let task = &all_tasks_arc[task_id];
                    let all_tasks_clone = all_tasks_arc.clone();
                    let blob_cache_clone = blob_cache.clone();
                    let commit_results_clone = commit_results.clone();
                    let file_measurer_clone = file_measurer.clone();
                    let overall_progress_clone = overall_progress.clone();

                    let aggregation_result_opt = task.result.get_or_init(|| {
                        Self::aggregate_task_children(task, &all_tasks_clone, &blob_cache_clone)
                    }); // end get_or_init

                    if task.is_commit_root {
                        if let Some((commit_oid_task, commit_date)) = task.commit_info {
                            if let Some(aggregation_result) = aggregation_result_opt.as_ref() {
                                match file_measurer_clone.summarize_tree_data(aggregation_result) {
                                    Ok(final_data) => {
                                        let commit_data = CommitData { oid: commit_oid_task, date: commit_date, data: final_data };
                                        commit_results_clone.insert(commit_oid_task, commit_data);
                                    }
                                    Err(e) => {
                                        eprintln!("Error summarizing tree data for commit {}: {}", commit_oid_task, e);
                                    }
                                }
                            } else {
                                eprintln!("Warning: Aggregation result was None for commit root task {:?} (commit {})", task.entry_key, commit_oid_task);
                            }
                        } else { eprintln!("Warning: Commit root task missing commit info: {:?}", task.entry_key); }
                    }
                    overall_progress_clone.inc(1);
                }); // end par_iter().for_each()
        } // end loop over depths

        overall_progress.finish_with_message("Finished processing all tasks");

        let parallel_elapsed_secs = parallel_start.elapsed().as_secs_f64();
        println!(
            "Parallel task execution: processed {} commits in {:.2} seconds",
            commit_results.len(),
            parallel_elapsed_secs
        );

        let mut commit_stats = Vec::with_capacity(commits_with_info.len());
        for (commit_oid, _, _) in &commits_with_info {
            if let Some(entry) = commit_results.remove(commit_oid) {
                commit_stats.push(entry.1);
            } else {
                eprintln!(
                    "Warning: No result found for commit {} in final collection.",
                    commit_oid
                );
            }
        }

        let total_elapsed_secs = measurement_start.elapsed().as_secs_f64();
        println!(
            "Total time for measurement: {:.2} seconds",
            total_elapsed_secs
        );

        //Leak these on purpose, the OS will clean up when the program exits
        //They take a long time to drop and we don't have to do it
        std::mem::forget(all_tasks_arc);
        std::mem::forget(blob_cache);

        Ok(commit_stats)
    }

    fn build_task_graph<'a>(
        commits_with_info: &[(ObjectId, ObjectId, gix::date::Time)],
        repo_caches: &'a RepoCacheData,
        path_glob: Option<&'a GlobMatcher>,
        empty_path_idx: FilepathIdx,
    ) -> BuildTaskGraphResult<F>
    where
        F: FileData + Debug + Clone + Send + Sync + 'static + PossiblyEmpty,
    {
        let RepoCacheData {
            oid_set,
            flat_tree,
            filepath_set,
            tree_entry_set,
            filename_set,
            ..
        } = repo_caches;

        let mut all_tasks: Vec<AggTask<F>> = Vec::new();
        let mut task_id_counter = 0;
        let mut task_id_map: AHashMap<EntryKey, usize> = AHashMap::new();
        let mut path_buffer = SmallVec::new();
        let mut tasks_by_depth: Vec<Vec<usize>> = Vec::new();

        let mut entries_to_process_set: HashSet<(AliasedPath, EntryIdx)> = HashSet::new();
        let mut task_creation_visited: HashSet<EntryKey> = HashSet::new();
        let mut globally_traversed_trees: HashSet<EntryKey> = HashSet::new();

        // --- Task Creation Phase ---
        let pb_commits = pb_default(commits_with_info.len());
        pb_commits.set_message("Building tasks");
        for (commit_oid, tree_oid, commit_date) in
            commits_with_info.iter().rev().progress_with(pb_commits)
        {
            let tree_oid_idx = oid_set.get_index_of(&(*tree_oid, Kind::Tree)).unwrap() as OidIdx;
            let Some(root_tree_entry) = flat_tree.get(&tree_oid_idx) else {
                eprintln!("Warning: Commit {} root tree OID index {} not found in flat_tree. Skipping commit.", commit_oid, tree_oid_idx);
                continue;
            };
            let root_tree_entry = root_tree_entry.unwrap_tree();
            let mut dfs_stack: Vec<(FilepathIdx, &TreeEntry)> = Vec::new();
            let mut creation_stack: Vec<(EntryKey, &TreeEntry)> = Vec::new();
            task_creation_visited.clear();

            dfs_stack.push((empty_path_idx, root_tree_entry));

            // 1. DFS Traversal to collect blobs and build dependency order for tasks
            while let Some((path_idx, tree)) = dfs_stack.pop() {
                let entry_key = EntryKey::new(tree.oid_idx, path_idx);
                let is_root_node_for_this_commit = path_idx == empty_path_idx;

                if !is_root_node_for_this_commit && globally_traversed_trees.contains(&entry_key) {
                    continue;
                }
                // Skip if already visited *within this commit's traversal*
                // `insert` returns false if the value was already present.
                if !task_creation_visited.insert(entry_key) {
                    continue;
                }

                // Add node to creation stack for post-order processing
                creation_stack.push((entry_key, tree));

                let parent_path_slice = filepath_set.get_index(path_idx as usize).unwrap();
                for &child_entry_idx in tree.children.iter() {
                    let Some(entry) = tree_entry_set.get_index(child_entry_idx as usize) else {
                        eprintln!("Warning: Child entry index {} not found in tree_entry_set for tree {}. Skipping.", child_entry_idx, tree.oid_idx);
                        continue;
                    };

                    path_buffer.clear();
                    path_buffer.extend_from_slice(parent_path_slice);
                    path_buffer.push(entry.filename_idx);

                    match entry.kind {
                        TreeChildKind::Blob => {
                            if path_glob.map_or(true, |glob| {
                                glob.is_match(aliased_path_to_string(filename_set, &path_buffer))
                            }) {
                                entries_to_process_set
                                    .insert((path_buffer.clone(), child_entry_idx));
                            }
                        }
                        TreeChildKind::Tree => {
                            let child_path_idx =
                                filepath_set.get_index_of(&path_buffer).unwrap() as FilepathIdx;
                            let child_entry_key = EntryKey::new(entry.oid_idx, child_path_idx);

                            if globally_traversed_trees.contains(&child_entry_key) {
                                continue;
                            }

                            // If we encounter the child tree again *within this commit's traversal*
                            // before processing it (cycle check handled by `task_creation_visited` at loop start),
                            // we don't need to push it again. However, the check at the start of the
                            // loop (`!task_creation_visited.insert(entry_key)`) already covers this.
                            // We only need to find the tree and push it onto the stack if it exists.
                            if let Some(child_tree) = flat_tree.get(&entry.oid_idx) {
                                dfs_stack.push((child_path_idx, child_tree.unwrap_tree()));
                            } else {
                                eprintln!("Warning: Child tree OID index {} (path: {:?}) not found in flat_tree. Skipping subtree.", entry.oid_idx, aliased_path_to_string(filename_set, &path_buffer));
                            }
                        }
                    }
                }
            } // end while dfs_stack.pop()
            globally_traversed_trees.extend(task_creation_visited.iter());

            // 2. Process creation_stack to create tasks in post-order
            while let Some((entry_key, tree)) = creation_stack.pop() {
                let is_current_commit_root = entry_key.path_idx == empty_path_idx;

                if !is_current_commit_root && task_id_map.contains_key(&entry_key) {
                    continue;
                }

                let task_id = task_id_counter;
                task_id_counter += 1;

                let mut children_info: Vec<(Either<EntryKey, usize>, u32, TreeChildKind)> =
                    Vec::with_capacity(tree.children.len());
                let parent_path = filepath_set.get_index(entry_key.path_idx as usize).unwrap();
                let mut max_depth = 0;
                for &child_entry_idx in tree.children.iter() {
                    let Some(entry) = tree_entry_set.get_index(child_entry_idx as usize) else {
                        /* Error */
                        continue;
                    };
                    let child_filename_idx = entry.filename_idx;
                    let child_kind = &entry.kind;

                    path_buffer.clear();
                    path_buffer.extend_from_slice(parent_path);
                    path_buffer.push(child_filename_idx);
                    let child_path_idx =
                        filepath_set.get_index_of(&path_buffer).unwrap() as FilepathIdx;
                    let child_entry_key = EntryKey::new(entry.oid_idx, child_path_idx);

                    match child_kind {
                        TreeChildKind::Blob => {
                            children_info.push((
                                Either::Left(child_entry_key),
                                child_filename_idx,
                                child_kind.clone(),
                            ));
                        }
                        TreeChildKind::Tree => {
                            match task_id_map.get(&child_entry_key) {
                                Some(&child_task_id) => {
                                    if let Some(child_task) = all_tasks.get(child_task_id) {
                                        max_depth = child_task.depth.max(max_depth);
                                    } else {
                                        eprintln!("Warning: Child task ID {} not found in all_tasks. Skipping.", child_task_id);
                                    }
                                    children_info.push((
                                        Either::Right(child_task_id),
                                        child_filename_idx,
                                        child_kind.clone(),
                                    ));
                                }
                                None => {
                                    eprintln!("Warning: Child task ID {} not found in task_id_map. Skipping.", child_entry_key.oid_idx);
                                    continue;
                                }
                            }
                        }
                    }
                }

                let task_depth = max_depth + 1;
                if task_depth >= tasks_by_depth.len() {
                    tasks_by_depth.resize_with(task_depth + 1, Vec::new);
                }
                tasks_by_depth[task_depth].push(task_id);

                let current_commit_info = if is_current_commit_root {
                    Some((*commit_oid, gix_time_to_chrono(*commit_date)))
                } else {
                    None
                };

                let task = AggTask {
                    id: task_id,
                    entry_key,
                    children_info,
                    is_commit_root: is_current_commit_root, // Use the boolean calculated above
                    commit_info: current_commit_info,
                    result: OnceLock::new(),
                    depth: task_depth,
                };

                all_tasks.push(task);
                task_id_map.insert(entry_key, task_id);
            } // end while creation_stack.pop()
        } // end commit loop

        BuildTaskGraphResult {
            all_tasks,
            tasks_by_depth,
            entries_to_process_set,
        }
    }

    fn batch_process_objects(
        &self,
        cache: &mut ResultCache<TreeDataCollection<F>>,
        entries_to_process: Vec<(AliasedPath, EntryIdx)>,
    ) {
        let start_time = Instant::now();
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
        *cache = entries_to_process
            .into_iter()
            .par_bridge()
            .progress_with(progress)
            .fold(
                AHashMap::new,
                |mut acc: AHashMap<EntryKey, Option<TreeDataCollection<F>>>, (path, entry_idx)| {
                    let path_str = aliased_path_to_string(filename_set, &path);
                    let repo: &Repository = tl.get_or(|| shared_repo.clone().to_thread_local());
                    let MyEntry { oid_idx, .. } =
                        *tree_entry_set.get_index(entry_idx as usize).unwrap();
                    let (oid, _kind) = oid_set.get_index(oid_idx).unwrap();
                    match self.file_measurer.measure_entry(repo, &path_str, oid) {
                        Ok(measurement) => {
                            let mut tree_collection: TreeDataCollection<F> = BTreeMap::new();
                            let path_idx = filepath_set.get_index_of(&path).unwrap_or_else(|| {
                                panic!("Did not find {:?} in filepath set", path)
                            }) as FilepathIdx;
                            tree_collection.insert(Either::Left(path_idx), measurement);
                            let entry_key = EntryKey::new(oid_idx, path_idx);
                            acc.insert(entry_key, Some(tree_collection));
                        }
                        Err(e) => {
                            eprintln!(
                                "Warning: Failed to measure entry {} ({}): {}",
                                path_str, oid, e
                            );
                        }
                    };
                    acc
                },
            )
            .reduce(
                AHashMap::new,
                |mut acc: AHashMap<EntryKey, Option<TreeDataCollection<F>>>, cur| {
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

    // Helper function to aggregate children results for a task
    fn aggregate_task_children(
        task: &AggTask<F>,
        all_tasks: &Arc<Vec<AggTask<F>>>,
        blob_cache: &Arc<ResultCache<TreeDataCollection<F>>>,
    ) -> Option<TreeDataCollection<F>> {
        let tree_agg = task
            .children_info
            .iter()
            .flat_map(|(child_key_or_id, _child_filename_idx, _child_kind)| {
                let results_for_child: Vec<(Either<FilepathIdx, u32>, F)> = match child_key_or_id {
                    Either::Left(blob_entry_key) => blob_cache
                        .get(blob_entry_key)
                        .and_then(|v| v.value().as_ref().cloned())
                        .map(|tdc| tdc.into_iter().collect::<Vec<_>>())
                        .unwrap_or_default(),
                    Either::Right(child_task_id) => {
                        all_tasks.get(*child_task_id)
                            .and_then(|child_task| child_task.result.get()) // Get the OnceLock inner value
                            .and_then(|opt_data| opt_data.as_ref()) // Get Option<&TreeDataCollection>
                            .map(|data_ref| {
                                // Map to cloned data if Some
                                data_ref
                                    .iter()
                                    .map(|(key, value)| (key.clone(), value.clone()))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_else(|| {
                                // Handle None cases (out of bounds or result not ready/failed)
                                eprintln!(
                                    "Warning: Could not get result for child task ID {} (parent task {})",
                                    child_task_id, task.id
                                );
                                Vec::new()
                            })
                    }
                };
                results_for_child.into_iter()
            })
            .collect::<TreeDataCollection<F>>();
        Some(tree_agg)
    }
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
