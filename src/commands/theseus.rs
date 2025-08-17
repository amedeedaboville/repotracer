use crate::blame::file_blame::Keyable;
use crate::blame::file_blame::LineNumber;
use crate::blame::FileBlame;
use crate::collectors::list_in_range::list_commits_with_granularity;
use crate::collectors::list_in_range::Granularity;
use ahash::AHashMap;
use anyhow::Result;
use std::collections::HashMap;

use gix::bstr::BString;
use gix::bstr::ByteSlice;
use gix::diff::object::TreeRefIter;
use gix::diff::tree_with_rewrites;
use gix::diff::tree_with_rewrites::Action;
use gix::diff::tree_with_rewrites::ChangeRef;
use gix::object::tree;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
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
    pub file_blames: AHashMap<BString, FileBlame<CohortKey>>,
}

impl<CohortKey> RepositoryBlameSnapshot<CohortKey>
where
    CohortKey: Keyable,
{
    pub fn new(commit_id: gix::ObjectId) -> Self {
        Self {
            commit_id,
            file_blames: AHashMap::new(),
        }
    }

    pub fn add_file(&mut self, path: &BString, total_lines: LineNumber, cohort: CohortKey) {
        let file_blame = FileBlame::new(total_lines, cohort);
        self.file_blames.insert(path.clone(), file_blame);
    }

    pub fn delete_file(&mut self, path: &BString) -> Option<FileBlame<CohortKey>> {
        self.file_blames.remove(path)
    }

    pub fn rename_file(&mut self, old_path: &BString, new_path: &BString) -> Result<(), String> {
        let file_blame = self
            .file_blames
            .remove(old_path)
            .ok_or_else(|| format!("File not found for rename: {:?}", old_path))?;
        self.file_blames.insert(new_path.clone(), file_blame);
        Ok(())
    }

    pub fn get_file_blame_mut(&mut self, path: &BString) -> Option<&mut FileBlame<CohortKey>> {
        self.file_blames.get_mut(path)
    }

    pub fn repository_cohort_stats(&self) -> AHashMap<CohortKey, u64>
    where
        CohortKey: Eq + std::hash::Hash + Send + Sync,
    {
        self.file_blames
            .par_iter()
            .map(|(_, file_blame)| file_blame.cohort_stats())
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

    pub fn apply_action(&mut self, action: SnapshotAction<CohortKey>) -> Result<(), String> {
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
            SnapshotAction::ApplyLineDiffs {
                location,
                line_diffs,
            } => {
                if let Some(file_blame) = self.get_file_blame_mut(&location) {
                    file_blame.apply_line_diffs(line_diffs);
                } else {
                    return Err(format!(
                        "Could not find blame info for path {:?}, most likely it was moved",
                        location
                    ));
                }
            }
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

pub enum SnapshotDiff<CohortKey>
where
    CohortKey: Keyable,
{
    Addition {
        location: BString,
        cohort: CohortKey,
        oid: gix::ObjectId,
    },
    Deletion {
        location: BString,
    },
    Modification {
        cohort: CohortKey,
        id: gix::ObjectId,
        previous_id: gix::ObjectId,
        location: BString,
        previous_mode: tree::EntryMode,
        new_mode: tree::EntryMode,
    },
    Rewrite {
        source_location: BString,
        location: BString,
    },
}

/// Represents an action to be applied to a RepositoryBlameSnapshot
/// after parallel processing of git diffs
#[derive(Debug, Clone)]
pub enum SnapshotAction<CohortKey>
where
    CohortKey: Copy + PartialEq,
{
    AddFile {
        location: BString,
        total_lines: LineNumber,
        cohort: CohortKey,
    },
    DeleteFile {
        location: BString,
    },
    ApplyLineDiffs {
        location: BString,
        line_diffs: Vec<(std::ops::Range<u32>, std::ops::Range<u32>, CohortKey)>, // (before, after, cohort)
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
    let weekly_commits = list_commits_with_granularity(&repo, Granularity::Weekly, None, None)?;
    let mut platform = repo.diff_resource_cache_for_tree_diff()?;
    let mut previous_tree: Option<gix::Tree> = None;
    let mut current_tree: gix::Tree;
    let mut current_snapshot = RepositoryBlameSnapshot::<u32>::new(weekly_commits[0].id);

    let progress_bar = ProgressBar::new(weekly_commits.len() as u64);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta_precise}) {per_sec:0.1} {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );
    progress_bar.set_message("Processing commits");

    for commit in progress_bar.wrap_iter(weekly_commits.iter()) {
        current_tree = commit.tree()?;
        let cohort = commit
            .time()
            .unwrap()
            .format(gix::date::time::CustomFormat::new("%Y"))
            .parse()
            .expect("Could not parse year of commit");

        let mut work_todo = Vec::new();
        let for_each =
            |change: ChangeRef<'_>| -> Result<Action, Box<dyn std::error::Error + Send + Sync>> {
                if !change.entry_mode().is_blob() {
                    return Ok(Action::Continue);
                }
                let work_to_push = match change {
                    ChangeRef::Addition { location, id, .. } => SnapshotDiff::Addition {
                        location: location.to_owned(),
                        cohort,
                        oid: id,
                    },
                    ChangeRef::Deletion { location, .. } => SnapshotDiff::Deletion {
                        location: location.to_owned(),
                    },
                    ChangeRef::Modification {
                        location,
                        previous_entry_mode,
                        previous_id,
                        entry_mode,
                        id,
                        ..
                    } => SnapshotDiff::Modification {
                        cohort,
                        id,
                        previous_id,
                        location: location.to_owned(),
                        previous_mode: previous_entry_mode,
                        new_mode: entry_mode,
                    },
                    ChangeRef::Rewrite {
                        location,
                        source_location,
                        ..
                    } => SnapshotDiff::Rewrite {
                        source_location: source_location.to_owned(),
                        location: location.to_owned(),
                    },
                };
                work_todo.push(work_to_push);
                Ok(Action::Continue)
            };
        let options = gix::diff::tree_with_rewrites::Options {
            location: Some(gix::diff::tree::recorder::Location::Path),
            rewrites: Some(gix::diff::Rewrites::default()),
        };

        let previous_tree_iter =
            TreeRefIter::from_bytes(previous_tree.as_ref().map_or(&[], |tree| &tree.data));
        let current_tree_iter = TreeRefIter::from_bytes(&current_tree.data);

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

        let safe_repo = repo.clone().into_sync();
        let repo_tl = ThreadLocal::new();
        let platform_tl = ThreadLocal::new();

        let snapshot_actions: Result<Vec<SnapshotAction<_>>, _> = work_todo
            .par_iter()
            .map(
                |change| -> Result<SnapshotAction<_>, Box<dyn std::error::Error + Send + Sync>> {
                    // Get thread-local repository and platform (reused per thread)
                    let thread_repo = repo_tl.get_or(|| safe_repo.clone().to_thread_local());
                    let thread_platform = platform_tl.get_or(|| {
                        std::cell::RefCell::new(
                            thread_repo.diff_resource_cache_for_tree_diff().unwrap(),
                        )
                    });

                    match change {
                        SnapshotDiff::Addition {
                            location,
                            cohort,
                            oid,
                        } => {
                            let blob = thread_repo.find_blob(*oid)?;
                            let content = &blob.data;
                            let num_lines = content.lines().count();
                            Ok(SnapshotAction::AddFile {
                                location: location.clone(),
                                total_lines: num_lines as LineNumber,
                                cohort: *cohort,
                            })
                        }
                        SnapshotDiff::Deletion { location } => Ok(SnapshotAction::DeleteFile {
                            location: location.clone(),
                        }),
                        SnapshotDiff::Modification {
                            cohort,
                            id,
                            previous_id,
                            location,
                            previous_mode,
                            new_mode,
                        } => {
                            let mut line_diffs = Vec::new();
                            let mut platform_borrow = thread_platform.borrow_mut();

                            // Mode-aware handling
                            if previous_mode != new_mode {
                                let prev_is_blob = previous_mode.is_blob();
                                let new_is_blob = new_mode.is_blob();
                                if !prev_is_blob && new_is_blob {
                                    // Treat as AddFile at this path
                                    let new_blob = thread_repo.find_blob(*id)?;
                                    let new_lines = new_blob.data.lines().count() as LineNumber;
                                    return Ok(SnapshotAction::AddFile {
                                        location: location.clone(),
                                        total_lines: new_lines,
                                        cohort: *cohort,
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
                                    line_diffs.push((before, after, *cohort));
                                },
                            );

                            Ok(SnapshotAction::ApplyLineDiffs {
                                location: location.clone(),
                                line_diffs,
                            })
                        }
                        SnapshotDiff::Rewrite {
                            source_location,
                            location,
                        } => Ok(SnapshotAction::RenameFile {
                            source_location: source_location.clone(),
                            location: location.clone(),
                        }),
                    }
                },
            )
            .collect();

        let snapshot_actions = match snapshot_actions {
            Ok(actions) => actions,
            Err(e) => {
                println!("Error processing changes in parallel: {}", e);
                continue;
            }
        };

        // Apply actions in dependency-safe order within this commit:
        // 1) Renames, 2) Additions, 3) Modifications, 4) Deletions
        let mut rename_actions = Vec::new();
        let mut add_actions = Vec::new();
        let mut modify_actions = Vec::new();
        let mut delete_actions = Vec::new();

        for action in snapshot_actions {
            match &action {
                SnapshotAction::RenameFile { .. } => rename_actions.push(action),
                SnapshotAction::AddFile { .. } => add_actions.push(action),
                SnapshotAction::ApplyLineDiffs { .. } => modify_actions.push(action),
                SnapshotAction::DeleteFile { .. } => delete_actions.push(action),
            }
        }

        for a in rename_actions {
            if let Err(e) = current_snapshot.apply_action(a) {
                println!("Error applying rename action: {}", e);
            }
        }
        for a in add_actions {
            if let Err(e) = current_snapshot.apply_action(a) {
                println!("Error applying add action: {}", e);
            }
        }
        for a in modify_actions {
            if let Err(e) = current_snapshot.apply_action(a) {
                println!("Error applying modify action: {}", e);
            }
        }
        for a in delete_actions {
            if let Err(e) = current_snapshot.apply_action(a) {
                println!("Error applying delete action: {}", e);
            }
        }

        if tree_changes.is_err() {
            println!("Error in tree changes {}", commit.id.to_string());
            println!("  {:?}", tree_changes.err().unwrap());
            continue;
        }
        previous_tree = Some(current_tree);
    }

    Ok(())
}
