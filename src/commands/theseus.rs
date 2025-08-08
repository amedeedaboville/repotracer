use crate::blame::FileBlame;
use crate::collectors::list_in_range::list_commits_with_granularity;
use crate::collectors::list_in_range::Granularity;
use crate::collectors::repo_cache_data::RepoCacheData;
use anyhow::Result;
use chrono::{DateTime, Utc};
use gix::bstr::BString;
use gix::bstr::ByteSlice;
use gix::diff::object::TreeRefIter;
use gix::diff::tree_with_rewrites;
use gix::diff::tree_with_rewrites::Action;
use gix::diff::tree_with_rewrites::ChangeRef;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/*
Git of theseus in Rust
The end result is a stacked graph.
Each "vertical strip" in the graph represents the state of the repo for a particular week.

So we need to get a Vec containing:
CohortInfo {timestamp, Map<YearCohor, u64>} (or similar)
For each week, we need to get the last commit in that week, and then diff it against the previous week to build
a Blame for the entire repo at that commit.
But we are building up the blame "incrementally" week-over-week, so as to do less work.

So working backwards again, for a particular commit, we need:
For each path in the repo at that time, a whole "blame". (Which is probably some kind of tree of ranges)
Let's call that for now:

CommitBlame {
  commit_id: ObjectId,
  blames: HashMap<AliasedPath, FileBlame>
}

To build a CommitBlame, we should take the previous CommitBlame and then edit it based on the diff between the two commits.
There are four things that can happen to a file between two commits:
* Added : We create a new FileBlame for the path, with a BlameRange for the entire file.
* Deleted : We delete the FileBlame entry entirely
* Modified : We edit the existing FileBlame to update the BlameRange based on the blob diff
* Renamed : We delete the old FileBlame entry and create a new one in the same place.
*/
/*
We are going to be tracking blame information for every file in the repo as we go
along the weekly commits.

For each commit, we are going to tree diff it with its previous (parent) commit.
For each entry in the tree diff, it can be either:
* Added
* Deleted
* Modified
* Renamed

For each commit, we are going to make a bag of what each of these changes does to the
FullRepoBlame. So each commit will have a vec of BlameEntryDiff.
A file added will have each line in that BlameEntryDiff being an addition from that commit.
A deleted filed will have each line in that BlameEntryDiff being a deletion from that commit.

We'll be able to do this commit diffing in parallel, building a full Vec of these Vec<BlameEntryDiff>s.

Then, at the end we can aggregate these to build a CommitFullBlame for each commit.
Then for each CommitFullBlame, we can filter that down to just the cohort information to build our theseus graph.

So we need two kinds of data (and possibly diff vs aggregated versions):

For a single commit, it's going to have a vec of "diffs", which are going to be BlameEntryDiff.
That's going to need to know which file, (original and new path), and then it's going to need a list of BlameRanges.

So for each commit we're building a Vec<BlameEntryDiff>, which is mostly the results of the tree diff between it and its parent.

Then, for the accumulating stage,
 */

// As if we had run git blame on every file in the repo at a given point in time
/*
#[derive(Debug, Clone)]
pub struct FullRepoBlameForCommit {
    pub commit_oid: ObjectId,
    pub blames: HashMap<AliasedPath, TheseusBlameEntry>,
}
//For this
#[derive(Debug, Clone)]
pub struct SingleFileBlame {
    pub blames: Vec<TheseusBlameEntry>,
}

impl TheseusBlameEntry {
    pub fn new(entry: BlameEntry, cohort: String, timestamp: Option<DateTime<Utc>>) -> Self {
        Self {
            entry,
            cohort,
            timestamp,
        }
    }

    pub fn line_count(&self) -> usize {
        self.entry.len.get() as usize
    }

    pub fn commit_id_string(&self) -> String {
        self.entry.commit_id.to_string()
    }

    pub fn start_line(&self) -> u32 {
        self.entry.start_in_blamed_file
    }

    pub fn end_line(&self) -> u32 {
        self.entry.start_in_blamed_file + self.entry.len.get()
    }
}
*/

/// Main theseus analysis configuration
#[derive(Debug, Clone)]
pub struct TheseusConfig {
    pub cohort_format: String, // e.g., "%Y" for yearly cohorts
    pub interval_days: u64,    // granularity in days (7 for weekly)
    pub branch: String,        // branch to analyze
}

/// Represents blame information for the entire repository at a specific commit
/// This is inspired by hercules' approach but structured for theseus analysis
#[derive(Debug, Clone)]
pub struct RepositoryBlameSnapshot<CohortKey>
where
    CohortKey: Copy + PartialEq,
{
    pub commit_id: gix::ObjectId,
    /// Map from file path to its blame information
    pub file_blames: HashMap<BString, FileBlame<CohortKey>>,
}

impl<CohortKey> RepositoryBlameSnapshot<CohortKey>
where
    CohortKey: Copy + PartialEq,
{
    pub fn new(commit_id: gix::ObjectId) -> Self {
        Self {
            commit_id,
            file_blames: HashMap::new(),
        }
    }

    /// Add a new file with initial blame information
    pub fn add_file(
        &mut self,
        path: &BString,
        total_lines: u32,
        commit_id: gix::ObjectId,
        commit_timestamp: DateTime<Utc>,
        cohort: CohortKey,
    ) {
        let file_blame = FileBlame::new(total_lines, commit_id, commit_timestamp, cohort);
        self.file_blames.insert(path.clone(), file_blame);
    }

    /// Remove a file from the blame tracking
    pub fn delete_file(&mut self, path: &BString) -> Option<FileBlame<CohortKey>> {
        self.file_blames.remove(path)
    }

    /// Rename a file (move blame information from old path to new path)
    pub fn rename_file(&mut self, old_path: &BString, new_path: &BString) -> Result<(), String> {
        let file_blame = self
            .file_blames
            .remove(old_path)
            .ok_or_else(|| format!("File not found for rename: {:?}", old_path))?;
        self.file_blames.insert(new_path.clone(), file_blame);
        Ok(())
    }

    /// Modify an existing file by updating its blame information
    /// This would typically involve applying line-level diffs
    pub fn modify_file(&mut self, path: &BString) -> Option<&mut FileBlame<CohortKey>> {
        self.file_blames.get_mut(path)
    }

    /// Get blame information for a specific file
    pub fn get_file_blame(&self, path: &BString) -> Option<&FileBlame<CohortKey>> {
        self.file_blames.get(path)
    }

    /// Generate cohort statistics across the entire repository
    pub fn repository_cohort_stats(&self) -> HashMap<CohortKey, u64>
    where
        CohortKey: Eq + std::hash::Hash,
    {
        let mut total_stats = HashMap::new();

        for file_blame in self.file_blames.values() {
            let file_stats = file_blame.cohort_stats();
            for (cohort, line_count) in file_stats {
                *total_stats.entry(cohort).or_insert(0) += line_count;
            }
        }

        total_stats
    }

    /// Get total lines across all files
    pub fn total_repository_lines(&self) -> u32 {
        self.file_blames
            .values()
            .map(|blame| blame.total_lines())
            .sum()
    }
}

impl Default for TheseusConfig {
    fn default() -> Self {
        Self {
            cohort_format: "%Y".to_string(),
            interval_days: 7,
            branch: "main".to_string(),
        }
    }
}

pub fn theseus_command(repo_path: &str) {
    println!("Starting Theseus analysis...");
    println!("Repository path: {}", repo_path);

    // Verify the path exists
    if !Path::new(repo_path).exists() {
        eprintln!("Error: Repository path does not exist or is not accessible.");
        return;
    }

    match run_theseus(repo_path) {
        Ok(_) => println!("Theseus completed successfully!"),
        Err(e) => eprintln!("Error running Theseus analysis: {}", e),
    }
}

fn run_theseus(repo_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config = TheseusConfig::default();

    println!("Configuration:");
    println!("  Cohort format: {}", config.cohort_format);
    println!("  Interval: {} days", config.interval_days);
    println!("  Branch: {}", config.branch);

    let repo = gix::open(repo_path)?;
    let weekly_commits = list_commits_with_granularity(&repo, Granularity::Weekly, None, None)?;
    println!("Found {} weekly commit snapshots", weekly_commits.len());
    let mut platform = repo.diff_resource_cache_for_tree_diff()?;
    let mut previous_commit_id: Option<gix::ObjectId> = None;
    let mut previous_tree_info: Option<TreeSnapshotInfo> = None;
    let mut current_tree_info: Option<TreeSnapshotInfo>;
    let mut current_commit_id: Option<gix::ObjectId>;
    let mut current_snapshot = RepositoryBlameSnapshot::new(weekly_commits[0].id);

    for (i, commit) in weekly_commits.iter().enumerate() {
        // if i > 2 {
        //     break;
        // }
        let current_commit = repo.find_commit(commit.id)?;
        current_commit_id = Some(commit.id);
        let current_tree = current_commit.tree()?;
        current_tree_info = Some(TreeSnapshotInfo {
            commit_id: commit.id,
            tree_id: current_tree.id().into(),
        });
        /*
        print!(
            "{}: Processing diff {:?} - {:?}",
            i + 1,
            previous_commit_id,
            current_commit_id
        );
        */

        let previous_tree = if let Some(previous_info) = &previous_tree_info {
            Some(repo.find_tree(previous_info.tree_id)?)
        } else {
            None
        };

        let mut tree_diff_state = gix::diff::tree::State::default();
        let mut objects = &repo.objects;
        let mut num_changes_for_commit = 0;
        let cohort: u32 = commit
            .time()
            .unwrap()
            .format(gix::date::time::CustomFormat::new("%Y"))
            .parse()
            .expect("Could not parse year of commit");
        let for_each =
            |change: ChangeRef<'_>| -> Result<Action, Box<dyn std::error::Error + Send + Sync>> {
                if !change.entry_mode().is_blob() {
                    return Ok(Action::Continue);
                }
                match change {
                    ChangeRef::Addition {
                        location,
                        entry_mode,
                        id,
                        ..
                    } => {
                        let blob = repo.find_blob(id)?;
                        let content = &blob.data;
                        let num_lines = content.lines().count();
                        current_snapshot.add_file(
                            &BString::from(location.to_str().unwrap()),
                            num_lines as u32,
                            id,
                            DateTime::from_timestamp(commit.time().unwrap().seconds, 0).unwrap(),
                            cohort,
                        );
                        /*
                        println!(
                            "  {:?} (path id {:?}) {} {:?}, (cohort {:?}, num lines {:?})",
                            location, path_idx, id, relation, cohort, num_lines
                        );
                        */
                        num_changes_for_commit += 1;
                    }
                    ChangeRef::Deletion {
                        location,
                        entry_mode,
                        ..
                    } => {
                        if !entry_mode.is_blob() {
                            return Ok(Action::Continue);
                        }

                        current_snapshot.delete_file(&BString::from(location.to_str().unwrap()));
                        /*
                        println!("  {:?}", change);
                         */
                        num_changes_for_commit += 1;
                    }
                    ChangeRef::Modification {
                        location,
                        previous_entry_mode,
                        previous_id,
                        entry_mode,
                        id,
                    } => {
                        // Only process blobs
                        if !entry_mode.is_blob() {
                            return Ok(Action::Continue);
                        }

                        /*
                        println!("  {:?}", change);
                         */
                        num_changes_for_commit += 1;
                    }
                    ChangeRef::Rewrite {
                        location,
                        source_location,
                        source_entry_mode,
                        source_relation,
                        source_id,
                        diff,
                        entry_mode,
                        id,
                        relation,
                        copy,
                    } => {
                        // Skip if this is a tree (directory) rather than a blob (file)
                        if entry_mode.is_tree() || source_entry_mode.is_tree() {
                            return Ok(Action::Continue);
                        }

                        /*
                        println!("  {:?}", change);
                         */
                        num_changes_for_commit += 1;
                    }
                }
                Ok(Action::Continue)
            };
        let options = gix::diff::tree_with_rewrites::Options {
            location: Some(gix::diff::tree::recorder::Location::Path),
            rewrites: Some(gix::diff::Rewrites::default()),
        };

        // Create TreeRefIter for previous tree (or empty iterator if None)
        let previous_tree_iter = if let Some(ref tree) = previous_tree {
            TreeRefIter::from_bytes(&tree.data)
        } else {
            TreeRefIter::from_bytes(&[]) // Empty tree iterator for the first commit
        };

        // Create TreeRefIter for current tree
        let current_tree_iter = TreeRefIter::from_bytes(&current_tree.data);

        let tree_changes = tree_with_rewrites(
            previous_tree_iter,
            current_tree_iter,
            &mut platform,
            &mut tree_diff_state,
            &mut objects,
            for_each,
            options,
        );

        if tree_changes.is_err() {
            println!("Error in tree changes {}", commit.id.to_string());
            println!("  {:?}", tree_changes.err().unwrap());
            continue;
        }
        println!(" num changes for commit: {} ", num_changes_for_commit,);

        previous_tree_info = current_tree_info;
        previous_commit_id = current_commit_id;
        println!(
            "Current snapshot: {:?}",
            current_snapshot.repository_cohort_stats()
        );
    }

    Ok(())
}

/// Represents a tree snapshot for diffing
#[derive(Debug, Clone)]
struct TreeSnapshotInfo {
    commit_id: gix::ObjectId,
    tree_id: gix::ObjectId,
}

/// Represents a change between two trees
#[derive(Debug, Clone)]
struct TreeChange {
    path: PathBuf,
    change_type: TreeChangeType,
    old_blob_id: Option<gix::ObjectId>,
    new_blob_id: Option<gix::ObjectId>,
}

/// Types of changes that can happen to files between trees
#[derive(Debug, Clone)]
enum TreeChangeType {
    Addition,
    Deletion,
    Modification,
    Rename { old_path: PathBuf },
}

/// Diff two trees and return the changes
fn diff_trees(
    repo: &gix::Repository,
    previous: &TreeSnapshotInfo,
    current: &TreeSnapshotInfo,
) -> Result<Vec<TreeChange>> {
    // For now, implement a simple manual diff by collecting all files from both trees
    // and comparing them. This is less efficient but will work as a starting point.

    let mut changes = Vec::new();
    let previous_tree = repo.find_tree(previous.tree_id)?;
    let current_tree = repo.find_tree(current.tree_id)?;

    // Collect files from previous tree
    let mut previous_files = std::collections::HashMap::new();
    collect_tree_files(&previous_tree, PathBuf::new(), &mut previous_files)?;

    // Collect files from current tree and detect changes
    let mut current_files = std::collections::HashMap::new();
    collect_tree_files(&current_tree, PathBuf::new(), &mut current_files)?;

    // Find additions and modifications
    for (path, current_oid) in &current_files {
        match previous_files.get(path) {
            None => {
                // File was added
                changes.push(TreeChange {
                    path: path.clone(),
                    change_type: TreeChangeType::Addition,
                    old_blob_id: None,
                    new_blob_id: Some(*current_oid),
                });
            }
            Some(previous_oid) => {
                if previous_oid != current_oid {
                    // File was modified
                    changes.push(TreeChange {
                        path: path.clone(),
                        change_type: TreeChangeType::Modification,
                        old_blob_id: Some(*previous_oid),
                        new_blob_id: Some(*current_oid),
                    });
                }
                // If they're equal, no change
            }
        }
    }

    // Find deletions
    for (path, previous_oid) in &previous_files {
        if !current_files.contains_key(path) {
            changes.push(TreeChange {
                path: path.clone(),
                change_type: TreeChangeType::Deletion,
                old_blob_id: Some(*previous_oid),
                new_blob_id: None,
            });
        }
    }

    Ok(changes)
}

/// Recursively collect all files from a tree
fn collect_tree_files(
    tree: &gix::Tree,
    base_path: PathBuf,
    files: &mut std::collections::HashMap<PathBuf, gix::ObjectId>,
) -> Result<()> {
    for entry in tree.iter() {
        let entry = entry?;
        let entry_path = base_path.join(entry.filename().to_os_str()?);

        if entry.mode().is_tree() {
            // Recursively process subdirectory
            let subtree = entry.object()?.into_tree();
            collect_tree_files(&subtree, entry_path, files)?;
        } else if entry.mode().is_blob() {
            // Add file to collection
            files.insert(entry_path, entry.oid().into());
        }
        // Skip other entry types (symlinks, etc.) for now
    }
    Ok(())
}
