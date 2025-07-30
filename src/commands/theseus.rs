use std::collections::HashMap;
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};
use anyhow::Result;
use gix_blame::{BlameEntry, Outcome as BlameOutcome};
use gix::bstr::ByteSlice;

/// Extended metadata for theseus analysis, wrapping gix-blame types
#[derive(Debug, Clone)]
pub struct TheseusBlameEntry {
    pub entry: BlameEntry,
    pub cohort: String, // year or custom format (e.g., "2023", "2023-Q1")
    pub timestamp: Option<DateTime<Utc>>, // We'll need to look up commit info separately
}

impl TheseusBlameEntry {
    pub fn new(entry: BlameEntry, cohort: String, timestamp: Option<DateTime<Utc>>) -> Self {
        Self { entry, cohort, timestamp }
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

/// Tracks blame information for a single file using gix-blame
#[derive(Debug, Clone)]
pub struct FileBlameInfo {
    pub path: PathBuf,
    pub entries: Vec<TheseusBlameEntry>,
}

impl FileBlameInfo {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            entries: Vec::new(),
        }
    }
    
    pub fn from_blame_outcome(path: PathBuf, outcome: BlameOutcome, _cohort_format: &str) -> Self {
        // For now, we'll create entries without timestamp lookup - this will be improved later
        let entries = outcome
            .entries
            .into_iter()
            .map(|entry| {
                // We'll need to implement commit timestamp lookup later
                let cohort = "unknown".to_string(); // Placeholder
                TheseusBlameEntry::new(entry, cohort, None)
            })
            .collect();
        
        Self { path, entries }
    }
    
    pub fn add_entry(&mut self, entry: TheseusBlameEntry) {
        self.entries.push(entry);
    }
    
    pub fn total_lines(&self) -> usize {
        self.entries.iter().map(|e| e.line_count()).sum()
    }
    
    pub fn lines_by_cohort(&self) -> HashMap<String, usize> {
        let mut cohort_lines = HashMap::new();
        for entry in &self.entries {
            *cohort_lines.entry(entry.cohort.clone()).or_insert(0) += entry.line_count();
        }  
        cohort_lines
    }
    
    pub fn lines_by_author(&self) -> HashMap<String, usize> {
        let mut author_lines = HashMap::new();
        for entry in &self.entries {
            let author = entry.commit_id_string(); // Simplified - use commit ID as author for now
            *author_lines.entry(author).or_insert(0) += entry.line_count();
        }
        author_lines
    }
}

/// Aggregates blame information for an entire tree/commit
#[derive(Debug, Clone)]
pub struct TreeBlameInfo {
    pub files: HashMap<PathBuf, FileBlameInfo>,
    pub timestamp: DateTime<Utc>,
}

impl TreeBlameInfo {
    pub fn new(timestamp: DateTime<Utc>) -> Self {
        Self {
            files: HashMap::new(),
            timestamp,
        }
    }
    
    pub fn add_file(&mut self, file_info: FileBlameInfo) {
        self.files.insert(file_info.path.clone(), file_info);
    }
    
    pub fn total_lines(&self) -> usize {
        self.files.values().map(|f| f.total_lines()).sum()
    }
    
    pub fn lines_by_cohort(&self) -> HashMap<String, usize> {
        let mut total_cohort_lines = HashMap::new();
        for file in self.files.values() {
            for (cohort, lines) in file.lines_by_cohort() {
                *total_cohort_lines.entry(cohort).or_insert(0) += lines;
            }
        }
        total_cohort_lines
    }
}

/// Represents changes to blame information between two states
#[derive(Debug, Clone)]
pub struct BlameInfoDiff {
    pub added_entries: Vec<TheseusBlameEntry>,
    pub removed_entries: Vec<TheseusBlameEntry>,
    pub file_path: PathBuf,
}

impl BlameInfoDiff {
    pub fn new(file_path: PathBuf) -> Self {
        Self {
            added_entries: Vec::new(),
            removed_entries: Vec::new(),
            file_path,
        }
    }
    
    pub fn add_entry(&mut self, entry: TheseusBlameEntry) {
        self.added_entries.push(entry);
    }
    
    pub fn remove_entry(&mut self, entry: TheseusBlameEntry) {
        self.removed_entries.push(entry);
    }
    
    pub fn net_lines_by_cohort(&self) -> HashMap<String, i64> {
        let mut cohort_changes = HashMap::new();
        
        // Add positive changes
        for entry in &self.added_entries {
            *cohort_changes.entry(entry.cohort.clone()).or_insert(0) += entry.line_count() as i64;
        }
        
        // Subtract removed changes
        for entry in &self.removed_entries {
            *cohort_changes.entry(entry.cohort.clone()).or_insert(0) -= entry.line_count() as i64;
        }
        
        cohort_changes
    }
}

/// Main theseus analysis configuration
#[derive(Debug, Clone)]
pub struct TheseusConfig {
    pub cohort_format: String, // e.g., "%Y" for yearly cohorts
    pub interval_days: u64,    // granularity in days (7 for weekly)
    pub branch: String,        // branch to analyze
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
    
    match run_theseus_analysis(repo_path) {
        Ok(_) => println!("Theseus analysis completed successfully!"),
        Err(e) => eprintln!("Error running Theseus analysis: {}", e),
    }
}

fn run_theseus_analysis(repo_path: &str) -> Result<()> {
    let config = TheseusConfig::default();
    
    println!("Configuration:");
    println!("  Cohort format: {}", config.cohort_format);
    println!("  Interval: {} days", config.interval_days);
    println!("  Branch: {}", config.branch);
    
    // 1. Open repository with gix
    let repo = gix::open(repo_path)?;
    println!("Opened repository: {}", repo_path);
    
    // 2. Walk commits at weekly intervals
    let weekly_commits = collect_weekly_commits(&repo, &config)?;
    println!("Found {} weekly commit snapshots", weekly_commits.len());
    
    // 3. Process each weekly snapshot with tree diffing
    let mut previous_tree_info: Option<TreeSnapshotInfo> = None;
    
    for (i, commit) in weekly_commits.iter().enumerate() {
        println!("Processing snapshot {}: {} ({})", 
                 i + 1, 
                 commit.id.to_string(), 
                 DateTime::from_timestamp(commit.time.seconds, 0)
                     .unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap())
                     .format("%Y-%m-%d"));
        
        // Get current tree
        let current_commit = repo.find_commit(commit.id)?;
        let current_tree = current_commit.tree()?;
        let current_tree_info = TreeSnapshotInfo {
            commit_id: commit.id,
            tree_id: current_tree.id,
            timestamp: commit.time,
        };
        
        // Diff against previous tree if we have one
        if let Some(previous_info) = &previous_tree_info {
            println!("  Diffing against previous snapshot...");
            let tree_changes = diff_trees(&repo, previous_info, &current_tree_info)?;
            println!("  Found {} file changes", tree_changes.len());
            
            // Process the changes
            for change in &tree_changes {
                match &change.change_type {
                    TreeChangeType::Addition => {
                        println!("    + Added: {}", change.path.display());
                    }
                    TreeChangeType::Deletion => {
                        println!("    - Deleted: {}", change.path.display());
                    }
                    TreeChangeType::Modification => {
                        println!("    ~ Modified: {}", change.path.display());
                    }
                    TreeChangeType::Rename { old_path } => {
                        println!("    > Renamed: {} -> {}", old_path.display(), change.path.display());
                    }
                }
            }
            
            // TODO: For each change, update blame information
        } else {
            println!("  First snapshot - establishing baseline");
            // TODO: Initialize blame information for all files in first tree
        }
        
        previous_tree_info = Some(current_tree_info);
    }
    
    println!("Tree diffing completed. Next: implement blame tracking and aggregation.");
    
    Ok(())
}

/// Collects commits at weekly intervals
fn collect_weekly_commits(repo: &gix::Repository, config: &TheseusConfig) -> Result<Vec<WeeklyCommit>> {
    // Try to find the specified branch, fall back to HEAD
    let mut reference = repo.find_reference(&config.branch)
        .or_else(|_| repo.find_reference("main"))
        .or_else(|_| repo.find_reference("master"))
        .or_else(|_| {
            match repo.head_ref()? {
                Some(head_ref) => Ok(head_ref),
                None => Err(anyhow::anyhow!("No HEAD reference found")),
            }
        })?;
    
    let commit_id = reference.peel_to_id_in_place()?;
    let mut weekly_commits = Vec::new();
    let mut last_timestamp: Option<i64> = None;
    let interval_seconds = config.interval_days * 24 * 60 * 60;
    
    // Walk commits from newest to oldest
    for commit_info in repo.rev_walk([commit_id]).all()? {
        let commit_info = commit_info?;
        let commit_id = commit_info.id;
        let commit = repo.find_commit(commit_id)?;
        let commit_time = commit.time()?;
        
        // Check if this commit is at least interval_days apart from the last one
        match last_timestamp {
            None => {
                // First commit, always include
                weekly_commits.push(WeeklyCommit {
                    id: commit_id,
                    time: commit_time,
                });
                last_timestamp = Some(commit_time.seconds);
            }
            Some(last_time) => {
                if last_time - commit_time.seconds >= interval_seconds as i64 {
                    weekly_commits.push(WeeklyCommit {
                        id: commit_id,
                        time: commit_time,
                    });
                    last_timestamp = Some(commit_time.seconds);
                }
            }
        }
    }
    
    // Reverse to get chronological order (oldest first)
    weekly_commits.reverse();
    
    Ok(weekly_commits)
}

/// Represents a commit at a weekly interval
#[derive(Debug, Clone)]
struct WeeklyCommit {
    id: gix::ObjectId,
    time: gix::date::Time,
}

/// Represents a tree snapshot for diffing
#[derive(Debug, Clone)]
struct TreeSnapshotInfo {
    commit_id: gix::ObjectId,
    tree_id: gix::ObjectId,
    timestamp: gix::date::Time,
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
    current: &TreeSnapshotInfo
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