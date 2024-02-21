use chrono::{DateTime, Utc};
use dashmap::DashMap;

use super::list_in_range::Granularity;
use super::repo_cache_data::{
    FilenameIdx, MyEntry, MyOid, RepoCacheData, TreeChildKind, TreeEntry,
};
use crate::collectors::list_in_range::list_commits_with_granularity;
use crate::stats::common::{
    FileMeasurement, MeasurementData, PossiblyEmpty, ReduceFrom, TreeDataCollection,
};
use crate::util::pb_style;
use ahash::AHashMap;
use gix::objs::Kind;
use gix::{ObjectId, Repository};

use indicatif::ProgressBar;
use indicatif::{ParallelProgressIterator, ProgressIterator};
use rayon::iter::ParallelIterator;
use rayon::prelude::*;
use smallvec::SmallVec;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::time::Instant;
use thread_local::ThreadLocal;

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

type ResultCache<T, F> = DashMap<(MyOid, Option<FilenameIdx>), MeasurementData<T, F>>;
pub struct CachedWalker<T, F> {
    repo_path: String,
    repo_caches: RepoCacheData,
    file_measurer: Box<dyn FileMeasurement<F>>,
    _marker: PhantomData<T>,
}
impl<TreeData, FileData> CachedWalker<TreeData, FileData>
where
    TreeData: Debug + Clone + Send + Sync + 'static + PossiblyEmpty + ReduceFrom<FileData>,
    FileData: Debug + Clone + Send + Sync + 'static + PossiblyEmpty,
{
    pub fn new(
        repo_path: String,
        file_measurer: Box<dyn FileMeasurement<FileData>>, // Changed type here
    ) -> Self {
        CachedWalker::<TreeData, FileData> {
            repo_caches: RepoCacheData::new(&repo_path),
            repo_path,
            file_measurer,
            _marker: PhantomData,
        }
    }

    pub fn walk_repo_and_collect_stats(
        &mut self,
        granularity: Granularity,
        range: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
    ) -> Result<Vec<CommitStat>, Box<dyn std::error::Error>> {
        let RepoCacheData {
            oid_set,
            flat_tree,
            repo_safe: safe_repo,
            ..
        } = &self.repo_caches;
        let inner_repo = safe_repo.clone().to_thread_local();
        let mut cache: DashMap<(MyOid, Option<FilenameIdx>), MeasurementData<TreeData, FileData>> =
            DashMap::with_capacity(flat_tree.len());
        self.batch_process_objects(&mut cache);
        let tree_processing_start = Instant::now();
        let commits_to_process =
            list_commits_with_granularity(&inner_repo, granularity, range.0, range.1)?;
        println!("Collecting file results into trees for each commit:");
        let commit_count = commits_to_process.len();

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
                let (res, _processed) = self
                    .measure_tree(SmallVec::new(), tree_entry, &cache)
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
    fn batch_process_objects(&self, cache: &mut ResultCache<TreeData, FileData>) {
        println!("Processing all blobs in the repo");
        let RepoCacheData {
            filename_cache,
            filename_set,
            oid_set,
            repo_safe: shared_repo,
            ..
        } = &self.repo_caches;
        let start_time = Instant::now();
        let progress = ProgressBar::new(oid_set.num_blobs as u64);
        progress.set_style(pb_style());
        let tl = ThreadLocal::new();
        *cache = oid_set
            .iter_blobs()
            .par_bridge()
            .progress_with(progress)
            .fold(
                AHashMap::new,
                |mut acc: AHashMap<
                    (MyOid, Option<FilenameIdx>),
                    MeasurementData<TreeData, FileData>,
                >,
                 (oid, _kind)| {
                    let repo: &Repository = tl.get_or(|| shared_repo.clone().to_thread_local());
                    let oid_idx = oid_set.get_index_of(&(*oid, Kind::Blob)).unwrap() as MyOid;
                    let Some(parent_trees) = filename_cache.get(&oid_idx) else {
                        // println!("No parent trees in filename cache for blob {oid_idx}");
                        return acc;
                    };
                    for filename_idx in parent_trees {
                        let parent_filename =
                            filename_set.get_index(*filename_idx as usize).unwrap();
                        match self.file_measurer.measure_entry(repo, parent_filename, oid) {
                            Ok(measurement) => {
                                acc.insert(
                                    (oid_idx, Some(*filename_idx)),
                                    MeasurementData::FileData(measurement),
                                );
                            }
                            Err(_) => {
                                // if *filename_idx == 181 || *filename_idx == 263 {
                                //     println!(
                                //         "Error with measurement for ({oid_idx},{filename_idx}), {e}",
                                //     );
                                // }
                            }
                        };
                    }
                    acc
                },
            )
            .reduce(
                AHashMap::new,
                |mut acc: AHashMap<
                    (MyOid, Option<FilenameIdx>),
                    MeasurementData<TreeData, FileData>,
                >,
                 cur| {
                    acc.extend(cur);
                    acc
                },
            )
            .into_iter()
            .collect();

        println!(
            "Entry for 6250 181: {:?}",
            cache.get(&(6250u32, Some(181u32)))
        );
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
        cache: &DashMap<(MyOid, Option<FilenameIdx>), MeasurementData<TreeData, FileData>>,
    ) -> Result<(TreeData, usize), Box<dyn std::error::Error>> {
        let RepoCacheData {
            repo_safe: repo,
            flat_tree,
            filename_set,
            tree_entry_set: entry_set,
            ..
        } = &self.repo_caches;
        if cache.contains_key(&(tree.oid_idx, None)) {
            return Ok((
                cache
                    .get(&(tree.oid_idx, None))
                    .unwrap()
                    .clone()
                    .unwrap_tree(),
                0,
            ));
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
                            false => Some((entry_path, MeasurementData::TreeData(child_result))),
                        }
                    }
                }
            })
            .collect::<TreeDataCollection<TreeData, FileData>>();
        let res: TreeData = TreeData::reduce(repo, child_results)?;
        cache.insert((tree.oid_idx, None), MeasurementData::TreeData(res.clone()));
        Ok((res, acc))
    }
}
