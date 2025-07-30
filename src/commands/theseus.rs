use std::collections::HashMap;
use std::path::{Path, PathBuf};
use chrono::{DateTime, Utc};
use anyhow::Result;
use gix_blame::{BlameEntry, Outcome as BlameOutcome};

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
    
    // 3. Process each weekly snapshot
    for (i, commit) in weekly_commits.iter().enumerate() {
        println!("Processing snapshot {}: {} ({})", 
                 i + 1, 
                 commit.id.to_string(), 
                 DateTime::from_timestamp(commit.time.seconds, 0)
                     .unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap())
                     .format("%Y-%m-%d"));
        
        // For now, just show we can access the commit
        // TODO: Implement tree diffing and blame tracking
    }
    
    println!("Analysis completed. Next: implement tree diffing and blame tracking.");
    
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