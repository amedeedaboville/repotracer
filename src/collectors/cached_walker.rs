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
    /// Processes a set of Git tree objects to find and gather all the paths to process.
    ///
    /// # Parameters
    /// * `tree_ids` - An iterable of Git tree ObjectIds (typically the tree references from commits)
    /// * `path_glob` - Optional glob pattern to filter files by path
    ///
    /// # Returns
    /// A vector of tuples containing (AliasedPath, OidIdx) for all relevant files
    fn gather_objects_to_process<I>(
        &self,
        tree_ids: I,
        path_glob: Option<GlobMatcher>,
    ) -> Result<Vec<(AliasedPath, EntryIdx)>, Box<dyn std::error::Error>>
    where
        I: IntoIterator<Item = ObjectId>,
        I::IntoIter: ExactSizeIterator,
    {
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

        let tree_ids_iter = tree_ids.into_iter();
        let tree_ids_len = tree_ids_iter.len();
        for tree_id in tree_ids_iter.progress_with(pb_default(tree_ids_len)) {
            let commit_tree_oid_idx =
                oid_set.get_index_of(&(tree_id, Kind::Tree)).unwrap() as OidIdx;
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
            oid_set,
            flat_tree,
            repo_safe: safe_repo,
            filepath_set,   // Needed for path calculations
            tree_entry_set, // Needed for child lookups
            filename_set,   // Needed for blob path matching
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

        let tree_processing_start = Instant::now();
        println!("Building task graph and identifying blobs via commit traversal:");
        let task_graph_start = Instant::now();
        let commit_count = num_commits;

        let mut all_tasks: Vec<AggTask<F>> = Vec::new();
        let mut task_id_counter = 0;
        let mut task_id_map: AHashMap<EntryKey, usize> = AHashMap::new();
        let mut path_buffer = SmallVec::new(); // Reusable buffer for path calculations
        let mut tasks_by_depth: Vec<Vec<usize>> = Vec::new();

        let mut entries_to_process_set: HashSet<(AliasedPath, EntryIdx)> = HashSet::new();
        let mut task_creation_visited: HashSet<EntryKey> = HashSet::new(); // Tracks visited nodes *within* a single commit's DFS pass
        let mut globally_traversed_trees: HashSet<EntryKey> = HashSet::new(); // Tracks trees already fully explored by *any* previous commit pass

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
            let commit_root_entry_key = EntryKey::new(tree_oid_idx, empty_path_idx);

            let mut dfs_stack: Vec<(FilepathIdx, &TreeEntry)> = Vec::new();
            let mut creation_stack: Vec<(EntryKey, &TreeEntry)> = Vec::new();

            dfs_stack.push((empty_path_idx, root_tree_entry));
            task_creation_visited.clear(); // Reset for this commit's internal DFS cycle detection

            // 1. DFS Traversal (with global skip)
            while let Some((path_idx, tree)) = dfs_stack.pop() {
                let entry_key = EntryKey::new(tree.oid_idx, path_idx);
                if globally_traversed_trees.contains(&entry_key) {
                    continue;
                }
                if !task_creation_visited.insert(entry_key) {
                    continue; // Avoid cycles within this single commit's DFS pass
                }

                // If we reached here, it means this node is being visited *either*
                // globally for the first time, *or* for the first time within this commit's DFS path.
                // We push it onto the creation stack for potential task creation later.
                creation_stack.push((entry_key, tree));

                let parent_path_slice = filepath_set.get_index(path_idx as usize).unwrap();

                // Process children (for blob collection and further DFS)
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
                            // Collect blobs only if the parent tree wasn't globally skipped
                            if path_glob.as_ref().map_or(true, |glob| {
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

                            // --- Check global traversal BEFORE pushing to stack ---
                            // No need to push/process children if they've been globally handled
                            if globally_traversed_trees.contains(&child_entry_key) {
                                continue;
                            }

                            // Also, don't push if it's already in this commit's visited set (cycle)
                            if !task_creation_visited.contains(&child_entry_key) {
                                if let Some(child_tree) = flat_tree.get(&entry.oid_idx) {
                                    dfs_stack.push((child_path_idx, child_tree.unwrap_tree()));
                                } else {
                                    eprintln!("Warning: Child tree OID index {} not found in flat_tree. Skipping subtree.", entry.oid_idx);
                                }
                            }
                        }
                    }
                }
            } // end while dfs_stack.pop()

            // ---- After DFS for this commit is complete, mark visited nodes as globally traversed ----
            // We only mark them *after* the full DFS for this commit ensures they weren't part of an internal cycle
            // that was skipped *before* full exploration. Add all keys from this commit's visited set.
            globally_traversed_trees.extend(task_creation_visited.iter());

            // 2. Process creation_stack to create tasks in post-order
            while let Some((entry_key, tree)) = creation_stack.pop() {
                // If a task for this EntryKey already exists (created by a previous commit),
                // we don't need to create it again. But we *do* need its ID for dependencies.
                if task_id_map.contains_key(&entry_key) {
                    continue;
                }

                // --- Task Creation Logic (remains the same) ---
                let task_id = task_id_counter;
                task_id_counter += 1;

                let mut children_info: Vec<(Either<EntryKey, usize>, u32, TreeChildKind)> =
                    Vec::with_capacity(tree.children.len());
                let parent_path = filepath_set.get_index(entry_key.path_idx as usize).unwrap();

                let mut max_depth = 0;
                for &child_entry_idx in tree.children.iter() {
                    let Some(entry) = tree_entry_set.get_index(child_entry_idx as usize) else {
                        eprintln!("Error: Child entry {} missing during task creation phase for parent {}. Task graph may be incomplete.", child_entry_idx, entry_key.oid_idx);
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
                            // Blobs don't have tasks, link directly to EntryKey
                            children_info.push((
                                Either::Left(child_entry_key),
                                child_filename_idx,
                                child_kind.clone(),
                            ));
                        }
                        TreeChildKind::Tree => {
                            // Find the child task ID (it *must* exist if code reached here,
                            // either created now or by a previous commit).
                            let child_task_id = *task_id_map.get(&child_entry_key).expect(
                                &format!("Tree child task ID missing for {:?} (parent {:?}) in post-order creation. This should not happen.", child_entry_key, entry_key)
                            );
                            // We need the depth from the actual task struct
                            if let Some(child_task) = all_tasks.get(child_task_id) {
                                max_depth = child_task.depth.max(max_depth);
                            } else {
                                // This case should be impossible if task_id_map lookup succeeded
                                eprintln!("Error: Could not find child task struct for ID {} derived from map.", child_task_id);
                            }

                            children_info.push((
                                Either::Right(child_task_id),
                                child_filename_idx,
                                child_kind.clone(),
                            ));
                        }
                    }
                }
                // ... rest of task creation (depth calculation, storing task, adding to task_id_map) ...
                let task_depth = max_depth + 1;
                if task_depth >= tasks_by_depth.len() {
                    tasks_by_depth.resize_with(task_depth + 1, Vec::new);
                }
                tasks_by_depth[task_depth].push(task_id);

                let is_commit_root = entry_key == commit_root_entry_key;
                let current_commit_info = if is_commit_root {
                    Some((*commit_oid, gix_time_to_chrono(*commit_date)))
                } else {
                    None
                };

                let task = AggTask {
                    id: task_id,
                    entry_key,
                    children_info,
                    is_commit_root,
                    commit_info: current_commit_info,
                    result: OnceLock::new(),
                    depth: task_depth,
                };

                all_tasks.push(task);
                task_id_map.insert(entry_key, task_id); // Add task ID to map
            } // end while creation_stack.pop()
        } // end commit loop

        println!(
            "Created {} tasks (across {} unique trees) and identified {} unique blobs in {:.2} secs",
            all_tasks.len(),
            globally_traversed_trees.len(), // Number of unique EntryKeys traversed
            entries_to_process_set.len(),
            task_graph_start.elapsed().as_secs_f64()
        );

        // --- Convert blob set to Vec and process them ---
        let entries_to_process_vec = entries_to_process_set.into_iter().collect::<Vec<_>>();
        // Sorting might still be beneficial for batch_process_objects cache locality if it reads sequentially
        // entries_to_process_vec.sort_by_key(|(_path, idx)| tree_entry_set.get_index(*idx as usize).map(|e| e.oid_idx).unwrap_or(OidIdx::MAX));
        self.batch_process_objects(&mut cache, entries_to_process_vec);

        // --- Parallel Task Execution Phase (Unchanged) ---
        println!("Executing {} tasks level-by-level:", all_tasks.len()); // Use len() from Vec directly
        let parallel_start = Instant::now();
        let commit_results = Arc::new(DashMap::<ObjectId, CommitData>::new());
        let file_measurer = self.file_measurer.clone();
        let all_tasks_arc = Arc::new(all_tasks); // Base Arc for tasks
        let blob_cache = Arc::new(cache); // Base Arc for blob cache
        let overall_progress = pb_default(all_tasks_arc.len());
        overall_progress.set_style(pb_style());

        for (depth, tasks_at_this_depth) in tasks_by_depth.iter().enumerate() {
            // Optional: Add progress message per depth level
            // overall_progress.set_message(format!("Processing depth {}", depth));
            tasks_at_this_depth
                .par_iter()
                .for_each(|&task_id| { // This closure captures original Arcs by reference implicitly
                    let task = &all_tasks_arc[task_id];
                     // Ensure clones are created *inside* the closure if needed for 'move'
                     let all_tasks_clone = all_tasks_arc.clone();
                     let blob_cache_clone = blob_cache.clone();
                     let commit_results_clone = commit_results.clone(); // Clone for commit results access
                     let file_measurer_clone = file_measurer.clone(); // Clone file measurer if needed inside get_or_init or summarize
                     let overall_progress_clone = overall_progress.clone(); // Clone progress bar

                    let aggregation_result = task.result.get_or_init(move || { // 'move' captures the *clones*
                        let tree_agg = task
                            .children_info
                            .iter()
                             // Use a separate move for flat_map's closure if necessary, or rely on captured clones
                            .flat_map(move |(child_key_or_id, _child_filename_idx, _child_kind)| {
                                let results_for_child: Vec<(Either<FilepathIdx, u32>, F)> = match child_key_or_id {
                                    Either::Left(blob_entry_key) => {
                                        // Prefer using get and Option methods over expect/unwrap inside parallel code
                                        blob_cache_clone.get(blob_entry_key)
                                            .and_then(|v| v.value().as_ref().cloned()) // Option<TDC<F>>
                                            .map(|tdc| tdc.into_iter().collect::<Vec<_>>())
                                            .unwrap_or_default() // Use Vec::default() which is an empty Vec
                                    }
                                    Either::Right(child_task_id) => {
                                        let child_task = &all_tasks_clone[*child_task_id];
                                         // Use get() which returns Option<&T>, then process the Option
                                         child_task.result.get()
                                             .and_then(|opt_data| opt_data.as_ref()) // Converts Option<&Option<TDC>> to Option<&TDC>
                                             .map(|data_ref| {
                                                 data_ref.iter()
                                                     .map(|(key, value)| (key.clone(), value.clone()))
                                                     .collect::<Vec<_>>() // Collect into Vec<(K, F)>
                                             })
                                             .unwrap_or_else(|| {
                                                  // Log error instead of panicking if possible, or handle differently
                                                  eprintln!(
                                                      "Dependency task {} result not available for task {} (entry: {:?}) at depth {}",
                                                      child_task_id, task.id, task.entry_key, depth // Add depth context
                                                  );
                                                  Vec::new() // Return empty vec on error
                                             })
                                    }
                                };
                                // Return the iterator from the Vec
                                results_for_child.into_iter()
                            }) // end flat_map
                            .collect::<TreeDataCollection<F>>();
                        Some(tree_agg)
                    }); // end get_or_init closure

                    if task.is_commit_root {
                        if let Some((commit_oid, commit_date)) = task.commit_info {
                            // Use file_measurer_clone captured by the outer closure
                             // Ensure aggregation_result is valid before unwrapping
                             if let Some(agg_res) = aggregation_result.as_ref() {
                                 match file_measurer_clone.summarize_tree_data(agg_res) {
                                     Ok(final_data) => {
                                         let commit_data = CommitData {
                                             oid: commit_oid,
                                             date: commit_date,
                                             data: final_data,
                                         };
                                          // Use commit_results_clone captured by the outer closure
                                         commit_results_clone.insert(commit_oid, commit_data);
                                     }
                                     Err(e) => {
                                         eprintln!("Error summarizing tree data for commit {}: {}", commit_oid, e);
                                     }
                                 }
                            } else {
                                 eprintln!("Warning: Aggregation result was None for commit root task {:?} (commit {})", task.entry_key, commit_oid);
                            }
                        } else {
                            eprintln!("Warning: Commit root task missing commit info: {:?}", task.entry_key);
                        }
                    }
                    // Use overall_progress_clone captured by the outer closure
                    overall_progress_clone.inc(1);
                }); // end par_iter().for_each()
        } // end loop over depths

        overall_progress.finish_with_message("Finished processing all tasks");
        let parallel_elapsed_secs = parallel_start.elapsed().as_secs_f64();
        println!(
            "Parallel task execution: processed {} commits in {:.2} seconds", // Use commit_count if defined or recalculate
            commit_results.len(), // Use the actual number of results collected
            parallel_elapsed_secs
        );

        let mut commit_stats = Vec::with_capacity(commits_with_info.len());
        // Ensure we iterate over the original commit list to maintain order if needed
        for (commit_oid, _, _) in &commits_with_info {
            // Iterate over the original list
            if let Some(entry) = commit_results.remove(commit_oid) {
                commit_stats.push(entry.1); // entry is a tuple (key, value)
            } else {
                // This might happen if a commit was skipped earlier due to missing tree, etc.
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
        std::mem::forget(all_tasks_arc);
        std::mem::forget(blob_cache);

        Ok(commit_stats)
    }

    fn batch_process_objects(
        &self,
        cache: &mut ResultCache<TreeDataCollection<F>>,
        entries_to_process: Vec<(AliasedPath, EntryIdx)>,
    ) {
        let start_time = Instant::now();
        println!("Processing {} blobs", entries_to_process.len());
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
                        Err(_) => {}
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

    fn _measure_tree_iterative(
        &self,
        root: &TreeEntry,
        cache: &ResultCache<TreeDataCollection<F>>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let RepoCacheData {
            flat_tree,
            tree_entry_set: entry_set,
            filename_set,
            filepath_set,
            ..
        } = &self.repo_caches;
        let empty_path_idx = filepath_set.get_index_of(&SmallVec::new()).unwrap() as FilepathIdx;
        let mut stack: Vec<(FilepathIdx, &TreeEntry)> = vec![(empty_path_idx, root)];
        let mut agg_stack: Vec<(FilepathIdx, &TreeEntry)> = Vec::new();
        let mut path_buffer = SmallVec::new();

        while let Some((path_idx, tree)) = stack.pop() {
            let entry_key = EntryKey::new(tree.oid_idx, path_idx);
            if cache.contains_key(&entry_key) {
                continue;
            }
            agg_stack.push((path_idx, tree));
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
                path_buffer.clear();
                path_buffer.extend_from_slice(filepath_set.get_index(path_idx as usize).unwrap());
                path_buffer.push(entry.filename_idx);
                let child_path_idx =
                    filepath_set.get_index_of(&path_buffer).unwrap() as FilepathIdx;
                let child_entry_key = EntryKey::new(entry.oid_idx, child_path_idx);
                if !cache.contains_key(&child_entry_key) {
                    let Some(child) = flat_tree.get(&entry.oid_idx) else {
                        panic!("Did not find {} in flat repo", entry.oid_idx)
                    };
                    stack.push((child_path_idx, child.unwrap_tree()));
                }
            }
        }
        while let Some((path_idx, tree)) = agg_stack.pop() {
            let entry_key = EntryKey::new(tree.oid_idx, path_idx);
            if cache.contains_key(&entry_key) {
                continue;
            }
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
                    path_buffer.clear();
                    path_buffer.extend_from_slice(&parent_path);
                    path_buffer.push(*filename_idx);
                    let child_path_idx =
                        filepath_set.get_index_of(&path_buffer).unwrap() as FilepathIdx;
                    let child_entry_key = EntryKey::new(*oid_idx, child_path_idx);
                    let child_result: Option<TreeDataCollection<F>> =
                        cache.get(&child_entry_key).and_then(|x| x.clone());
                    match child_result {
                        Some(x) => Some((*filename_idx, x)),
                        None => None,
                    }
                })
                // .flatten()
                // Prepend the current filename to each result
                .flat_map(|(_filename_idx, data)| {
                    let mut new_path = parent_path.clone(); //temp array to build a new path, reused for each child
                    data.into_iter().map(move |(key, value)| {
                        let new_path_idx = match key {
                            Either::Left(_path_idx) => key,
                            Either::Right(leaf_filename_idx) => {
                                new_path.push(leaf_filename_idx);
                                let new_path_idx =
                                    filepath_set.get_index_of(&new_path).unwrap_or_else(|| {
                                        panic!(
                                            "Did not find new path in filepath set: {:?} ({})",
                                            new_path,
                                            aliased_path_to_string(filename_set, &new_path)
                                        )
                                    }) as FilepathIdx;
                                new_path.pop();
                                Either::Left(new_path_idx)
                            }
                        };
                        (new_path_idx, value)
                    })
                })
                .collect::<TreeDataCollection<F>>();
            cache.entry(entry_key).or_insert(Some(tree_agg));
        }

        let root_entry_key = EntryKey::new(root.oid_idx, empty_path_idx);
        cache
            .get(&root_entry_key)
            .map(|x| x.as_ref().map(|_| ()))
            .flatten()
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
