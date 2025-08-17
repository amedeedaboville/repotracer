use crate::blame::file_blame::Keyable;
use crate::blame::file_blame::LineNumber;
use crate::blame::FileBlame;
use crate::collectors::list_in_range::list_commits_with_granularity;
use crate::collectors::list_in_range::Granularity;
use ahash::AHashMap;
use anyhow::Result;
use dashmap::DashMap;
use gix::bstr::BString;
use gix::bstr::ByteSlice;
use gix::diff::object::TreeRefIter;
use gix::diff::tree_with_rewrites;
use gix::diff::tree_with_rewrites::Action;
use gix::diff::tree_with_rewrites::Change;
use gix::diff::tree_with_rewrites::ChangeRef;
use gix::object::tree;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use thread_local::ThreadLocal;

#[derive(Debug, Clone)]
pub struct TheseusConfig {
    pub cohort_format: String, // e.g., "%Y" for yearly cohorts
    pub interval_days: u64,    // granularity in days (7 for weekly)
    pub branch: String,        // branch to analyze
}

/// Represents blame information for the entire repository at a specific commit
#[derive(Debug, Clone)]
pub struct RepositoryBlameSnapshot<CohortKey>
where
    CohortKey: Keyable,
{
    pub commit_id: gix::ObjectId,
    pub file_blames: DashMap<BString, FileBlame<CohortKey>>,
}

impl<CohortKey> RepositoryBlameSnapshot<CohortKey>
where
    CohortKey: Keyable,
{
    pub fn new(commit_id: gix::ObjectId) -> Self {
        Self {
            commit_id,
            file_blames: DashMap::new(),
        }
    }

    fn add_file(&self, path: &BString, total_lines: LineNumber, cohort: CohortKey) {
        let file_blame = FileBlame::new(total_lines, cohort);
        self.file_blames.insert(path.clone(), file_blame);
    }

    fn delete_file(&self, path: &BString) -> Option<FileBlame<CohortKey>> {
        self.file_blames.remove(path).map(|(_, v)| v)
    }

    fn rename_file(&self, old_path: &BString, new_path: &BString) -> Result<(), String> {
        let file_blame = self
            .file_blames
            .remove(old_path)
            .ok_or_else(|| format!("File not found for rename: {:?}", old_path))?;
        self.file_blames.insert(new_path.clone(), file_blame.1);
        Ok(())
    }

    fn modify_file(
        &self,
        location: &BString,
        new_blame: FileBlame<CohortKey>,
    ) -> Result<(), String> {
        if let Some(mut blame) = self.file_blames.get_mut(location) {
            *blame = new_blame;
            Ok(())
        } else {
            Err(format!("File blame not found for {:?}", location))
        }
    }

    pub fn get_file_blame(&self, path: &BString) -> Option<FileBlame<CohortKey>> {
        self.file_blames.get(path).map(|r| r.value().clone())
    }

    pub fn repository_cohort_stats(&self) -> AHashMap<CohortKey, u64>
    where
        CohortKey: Eq + std::hash::Hash + Send + Sync,
    {
        self.file_blames
            .par_iter()
            .map(|ref_multi| ref_multi.value().cohort_stats())
            .reduce(
                || HashMap::new(),
                |mut acc, file_stats| {
                    for (cohort, line_count) in file_stats {
                        *acc.entry(cohort).or_insert(0) += line_count;
                    }
                    acc
                },
            )
            .into_iter()
            .collect()
    }

    pub fn apply_action(&self, action: SnapshotAction<CohortKey>) -> Result<(), String> {
        match action {
            SnapshotAction::AddFile {
                location,
                total_lines,
                cohort,
            } => {
                self.add_file(&location, total_lines, cohort);
            }
            SnapshotAction::DeleteFile { location } => {
                self.delete_file(&location);
            }
            SnapshotAction::UpdateFile {
                location,
                new_blame,
            } => self.modify_file(&location, new_blame)?,
            SnapshotAction::RenameFile {
                source_location,
                location,
            } => {
                self.rename_file(&source_location, &location)?;
            }
        }
        Ok(())
    }
}

/// Represents an action to be applied to a RepositoryBlameSnapshot
/// after parallel processing of git diffs
/// Maybe we could simply clone gix's ChangeRef instead, but this works for now
#[derive(Debug, Clone)]
pub enum SnapshotAction<CohortKey>
where
    CohortKey: Keyable,
{
    AddFile {
        location: BString,
        total_lines: LineNumber,
        cohort: CohortKey,
    },
    DeleteFile {
        location: BString,
    },
    UpdateFile {
        location: BString,
        new_blame: FileBlame<CohortKey>,
    },
    RenameFile {
        source_location: BString,
        location: BString,
    },
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
    let safe_repo = repo.clone().into_sync();
    let weekly_commits = list_commits_with_granularity(&repo, Granularity::Weekly, None, None)?;
    let mut platform = repo.diff_resource_cache_for_tree_diff()?;
    let mut previous_tree_data = Vec::new();
    let current_snapshot = RepositoryBlameSnapshot::<u32>::new(weekly_commits[0].id);

    let repo_tl = ThreadLocal::new();
    let platform_tl = ThreadLocal::new();
    //initialize thread-local repos and platforms
    rayon::broadcast(|_| {
        let repo = repo_tl.get_or(|| safe_repo.clone()).to_thread_local();
        platform_tl
            .get_or(|| std::cell::RefCell::new(repo.diff_resource_cache_for_tree_diff().unwrap()));
    });

    let progress_bar = ProgressBar::new(weekly_commits.len() as u64);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta_precise}) {per_sec:0.1} {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );
    progress_bar.set_message("Processing commits");
    // We do this detaching serially, so that we can look up the commit
    // data concurrently afterwards
    let detached_commits = weekly_commits
        .into_iter()
        .map(|c| c.detach())
        .collect::<Vec<_>>();
    let commits_with_info = detached_commits
        .into_par_iter()
        .map(|commit| {
            let repo = repo_tl.get().unwrap().to_thread_local();
            let commit = commit.attach(&repo).into_commit();
            let cohort = commit
                .time()
                .unwrap()
                .format(gix::date::time::CustomFormat::new("%Y"))
                .parse::<u32>()
                .expect("Could not parse year of commit");
            let tree = commit.tree().unwrap().detach();
            (tree.data, cohort)
        })
        .collect::<Vec<_>>();
    for (tree_data, cohort) in progress_bar.wrap_iter(commits_with_info.into_iter()) {
        let mut work_todo = Vec::new();
        let for_each =
            |change: ChangeRef<'_>| -> Result<Action, Box<dyn std::error::Error + Send + Sync>> {
                if !change.entry_mode().is_blob() {
                    return Ok(Action::Continue);
                }
                work_todo.push(change.into_owned());
                Ok(Action::Continue)
            };
        let options = gix::diff::tree_with_rewrites::Options {
            location: Some(gix::diff::tree::recorder::Location::Path),
            rewrites: Some(gix::diff::Rewrites::default()),
        };

        let previous_tree_iter = TreeRefIter::from_bytes(previous_tree_data.as_slice());
        let current_tree_iter = TreeRefIter::from_bytes(tree_data.as_slice());

        let mut objects = &repo.objects;
        let mut tree_diff_state = gix::diff::tree::State::default();

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
            return Err(tree_changes.err().unwrap().into());
        }

        work_todo
            .par_iter()
            .map(
                |change| -> Result<SnapshotAction<_>, Box<dyn std::error::Error + Send + Sync>> {
                    let thread_repo = repo_tl.get().unwrap().to_thread_local();
                    let thread_platform = platform_tl.get().unwrap();

                    match change {
                        Change::Addition { location, id, .. } => {
                            let blob = thread_repo.find_blob(*id)?;
                            let content = &blob.data;
                            let num_lines = content.lines().count();
                            Ok(SnapshotAction::AddFile {
                                location: location.clone(),
                                total_lines: num_lines as LineNumber,
                                cohort,
                            })
                        }
                        Change::Deletion { location, .. } => Ok(SnapshotAction::DeleteFile {
                            location: location.clone(),
                        }),
                        Change::Modification {
                            location,
                            previous_entry_mode,
                            previous_id,
                            entry_mode,
                            id,
                        } => {
                            let mut line_diffs = Vec::new();
                            let mut platform_borrow = thread_platform.borrow_mut();

                            if previous_entry_mode != entry_mode {
                                let prev_is_blob = previous_entry_mode.is_blob();
                                let new_is_blob = entry_mode.is_blob();
                                if !prev_is_blob && new_is_blob {
                                    // Treat as AddFile at this path
                                    let new_blob = thread_repo.find_blob(*id)?;
                                    let new_lines = new_blob.data.lines().count() as LineNumber;
                                    return Ok(SnapshotAction::AddFile {
                                        location: location.clone(),
                                        total_lines: new_lines,
                                        cohort,
                                    });
                                } else if prev_is_blob && !new_is_blob {
                                    // Treat as DeleteFile at this path
                                    return Ok(SnapshotAction::DeleteFile {
                                        location: location.clone(),
                                    });
                                } else {
                                    // Both blob (replacement) or both non-blob. If both blob, fall through to normal diff.
                                    if !prev_is_blob && !new_is_blob {
                                        // Non-blob → non-blob: ignore for blame by returning a no-op deletion (will do nothing if absent)
                                        return Ok(SnapshotAction::DeleteFile {
                                            location: location.clone(),
                                        });
                                    }
                                }
                            }

                            platform_borrow.set_resource(
                                *previous_id,
                                gix::object::tree::EntryKind::Blob,
                                location.as_ref(),
                                gix::diff::blob::ResourceKind::OldOrSource,
                                &thread_repo.objects,
                            )?;
                            platform_borrow.set_resource(
                                *id,
                                gix::object::tree::EntryKind::Blob,
                                location.as_ref(),
                                gix::diff::blob::ResourceKind::NewOrDestination,
                                &thread_repo.objects,
                            )?;

                            let outcome = platform_borrow.prepare_diff()?;
                            let input = outcome.interned_input();
                            gix::diff::blob::diff(
                                gix::diff::blob::Algorithm::Myers,
                                &input,
                                |before: std::ops::Range<u32>, after: std::ops::Range<u32>| {
                                    line_diffs.push((before, after, cohort));
                                },
                            );
                            let current_blame =
                                current_snapshot.get_file_blame(&location).ok_or_else(|| {
                                    format!("File blame not found for {:?}", location)
                                })?;
                            let new_blame = current_blame.apply_line_diffs(line_diffs);

                            Ok(SnapshotAction::UpdateFile {
                                location: location.clone(),
                                new_blame,
                            })
                        }
                        Change::Rewrite {
                            source_location,
                            location,
                            ..
                        } => Ok(SnapshotAction::RenameFile {
                            source_location: source_location.clone(),
                            location: location.clone(),
                        }),
                    }
                },
            )
            .for_each(|r| {
                let _ = current_snapshot.apply_action(r.unwrap());
            });

        // We need to clear the diff cache every so often.
        // Clearing it every 2, 10, 100 or 200 commits has nearly the same performance improvement,
        // a bit less than 10s, but consumes 60+ GB of RAM compared to  200MB for every commit.
        //  Clearing it less often than every commit is not worth it.
        rayon::broadcast(|_| {
            let mut platform = platform_tl.get().unwrap().borrow_mut();
            platform.clear_resource_cache();
        });
        previous_tree_data = tree_data;
    }

    Ok(())
}
