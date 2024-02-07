use indicatif::{ParallelProgressIterator, ProgressIterator};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use rayon::prelude::*;
use thread_local::ThreadLocal;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::cell::RefCell;
use std::sync::Arc;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use git2::Repository;

use std::fmt::Debug;

use crate::stats::common::{
    Either, FileMeasurement, PathMeasurement, TreeDataCollection, TreeReducer,
};

fn count_commits(repo: &Repository) -> Result<usize, git2::Error> {
    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?; // Start from HEAD
    revwalk.simplify_first_parent()?;

    let commit_count = revwalk.count();
    Ok(commit_count)
}

pub trait BlobMeasurer<F>: Sync
where
    F: Send + Sync,
{
    fn measure_entry(
        &self,
        repo: &Repository,
        path: &str,
        entry: &git2::TreeEntry,
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
        entry: &git2::TreeEntry,
    ) -> Result<F, Box<dyn std::error::Error>> {
        let obj = entry.to_object(repo).unwrap();
        let blob = obj.as_blob().unwrap();
        let content = std::str::from_utf8(blob.content()).unwrap_or_default();
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
        entry: &git2::TreeEntry,
    ) -> Result<F, Box<dyn std::error::Error>> {
        self.callback.measure_path(repo, &path)
    }
    fn measure_data(&self, _contents: &[u8]) -> Result<F, Box<dyn std::error::Error>> {
        unimplemented!()
    }
}
pub struct CachedWalker<T, F> {
    cache: HashMap<git2::Oid, Either<T, F>>,
    repo_path: String,
    file_measurer: Box<dyn BlobMeasurer<F>>,
    tree_reducer: Box<dyn TreeReducer<T, F>>,
}
impl<T, F> CachedWalker<T, F>
where
    T: Debug + Clone + Send + Sync,
    F: Debug + Clone + Send + Sync,
{
    pub fn new(
        repo_path: String,
        file_measurer: Box<dyn BlobMeasurer<F>>, // Changed type here
        tree_reducer: Box<dyn TreeReducer<T, F>>,
    ) -> Self {
        CachedWalker {
            cache: HashMap::new(),
            repo_path,
            file_measurer,
            tree_reducer,
        }
    }
    pub fn walk_repo_and_collect_stats(&mut self) -> Result<(), git2::Error> {
        let inner_repo = Repository::open(&self.repo_path)?;
        println!("Filling cache.");
        self.fill_cache(&inner_repo);

        let commit_count = count_commits(&inner_repo)?;
        println!("Found {commit_count} commits. Starting to walk the repo.");
        let mut revwalk = inner_repo.revwalk()?;
        revwalk.push_head()?;
        // Do earlier objects first
        // revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)?;
        revwalk.simplify_first_parent()?;

        let mut commit_stats = vec![];
        let mut i = 0;
        let progress_bar = ProgressBar::new(commit_count as u64);
        if commit_count <= 10_000 {
            progress_bar.set_draw_target(ProgressDrawTarget::hidden());
        } else {
            progress_bar.set_style(ProgressStyle::default_bar().template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta})  {msg}",
        ).expect("Failed to set progress bar style"));
        }

        let start_time = Instant::now();
        let alpha = 0.3; // Weighting factor for the EWMA
        let mut ewma_elapsed = Duration::new(0, 0); // Start with an EWMA elapsed time of 0
        let mut last_batch_start = Instant::now(); // Time when the last batch started
        let batch_size = 100; // Update the progress bar every 10 iterations

        let mut objects_procssed = 0;
        let mut objects_procssed_total = 0;
        for oid in revwalk {
            let Ok(oid) = oid else {
                let e = oid.unwrap_err();
                println!("Error with commit: {:?}", e);
                continue;
            };
            i += 1;

            if i % batch_size == 0 && i > 0 {
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
            let tree = inner_repo.find_commit(oid)?.tree()?;
            // let inner_repo = Repository::open(self.repo.path())?;
            let (res, processed) = self.measure_tree("", &tree, &inner_repo).unwrap();
            objects_procssed += processed;
            objects_procssed_total += processed;
            commit_stats.push((oid.to_string(), res));
        }
        let elapsed_secs = start_time.elapsed().as_secs_f64();
        if elapsed_secs > 0.0 {
            println!(
                "processed: {i} commits and {objects_procssed_total} objects in {elapsed_secs}, {:.2} objects/sec",
                objects_procssed_total as f64 / elapsed_secs
            );
        } else {
            println!("processed: {i} commits and {objects_procssed_total} objects");
        }
        for (oid, stats) in commit_stats.iter() {
            // println!("{oid}, {:?}", stats);
        }
        Ok(())
    }
    fn fill_cache(&mut self, repo: &Repository) {
        let odb = repo.odb().unwrap();
        let start_time = Instant::now();

        // Collect all OIDs into a Vec
        let mut oids: Vec<git2::Oid> = Vec::new();
        odb.foreach(|oid| {
            oids.push(*oid);
            true
        })
        .expect("Failed to iterate over object database");

        let style = ProgressStyle::default_bar().template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {per_sec}").expect("error with progress bar style");
        let progress = ProgressBar::new(oids.len() as u64);
        progress.set_style(style);
        println!("Processing {} files", oids.len());
        // let tl = ThreadLocal::new();
        let tl = Arc::new(ThreadLocal::new());
        let path = self.repo_path.clone();
        self.cache = oids
            .par_iter()
            // .with_min_len(1000) // Adjust the chunk size as needed
            // .progress_count(oids.len() as u64)
            .progress_with(progress)
            .filter_map(|oid| {
                let repo = tl.get_or(|| {
                    // Open a new Repository instance for each thread.
                    // Repository::open(&path).expect("Failed to open repository").odb().expect()
                    // let odb = repo.odb().expect("Failed to open object database");

                    // Since both `repo` and `odb` are created inside the closure, there's no issue with lifetimes here.
                    Repository::open(&path).expect("Failed to open repository")
                });
                let odb = repo.odb().unwrap();
                // .filter_map(|oid| {
                let obj = odb.read(*oid).unwrap();
                if obj.kind() == git2::ObjectType::Blob {
                    let contents = obj.data();
                    let res = self.file_measurer.measure_data(contents).unwrap();
                    Some((*oid, Either::Right(res)))
                } else {
                    None
                }
            })
            .collect::<HashMap<git2::Oid, Either<T, F>>>();

        println!(
            "Cache filled in {} seconds, contains {} items",
            start_time.elapsed().as_secs(),
            self.cache.len()
        );
    }
    fn measure_tree(
        &mut self,
        path: &str,
        tree: &git2::Tree,
        repo: &Repository,
    ) -> Result<(T, usize), Box<dyn std::error::Error>> {
        if self.cache.contains_key(&tree.id()) {
            return Ok((self.cache.get(&tree.id()).unwrap().clone().unwrap_left(), 0));
        }
        let mut acc = 0;
        let child_results = tree
            .iter()
            .filter_map(|entry| {
                let entry_name = entry.name().unwrap().to_string();
                if self.cache.contains_key(&entry.id()) {
                    return Some((entry_name, self.cache.get(&entry.id()).unwrap().clone()));
                }
                match entry.kind() {
                    Some(git2::ObjectType::Tree) => {
                        let child_object = entry.to_object(&repo).unwrap();
                        let (child_result, processed) = self
                            .measure_tree(
                                &format!("{path}/{entry_name}"),
                                &child_object.as_tree().unwrap(),
                                repo,
                            )
                            .unwrap();

                        let r = Either::Left(child_result);
                        acc += processed;
                        Some((entry_name, r))
                    }
                    Some(git2::ObjectType::Blob) => {
                        acc += 1;
                        let child_result = self
                            .file_measurer
                            .measure_entry(repo, path, &entry)
                            .map(Either::Right)
                            .unwrap();
                        Some((entry_name, child_result))
                    }
                    Some(git2::ObjectType::Commit) => {
                        println!("Warning: skipping submodule commit '{}' in tree '{entry_name}'", tree.id());
                        None
                    }
                    _  => {
                        println!("Warning: skiping unsupported git object type {:?} under '{entry_name}' in tree {}", entry.kind(), tree.id());
                        None
                    }
                }
            })
            .collect::<TreeDataCollection<T, F>>();
        let res = self.tree_reducer.reduce(repo, child_results)?;
        self.cache.insert(tree.id(), Either::Left(res.clone()));
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
