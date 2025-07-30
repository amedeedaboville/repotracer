use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::commands::guess_tools::{detect_tools_command, ToolMatchDetails};
use crate::config;
use crate::repo;

const TOOL_REPORT_MAX_AGE_DAYS: u64 = 30;

pub fn sync_command() -> Result<()> {
    let config = config::global_config();

    println!("Starting repository sync...");

    for (repo_name, repo_config) in &config.repos {
        println!("Processing repository: {}", repo_name);

        // First, ensure the repository is cloned/updated
        if let Some(url) = get_repo_url(repo_config) {
            println!("  Syncing repository from: {}", url);
            if let Err(e) = repo::clone_repo(&url) {
                eprintln!("  Warning: Failed to sync repository {}: {}", repo_name, e);
                continue;
            }
        }

        // Check if we need to run tool detection
        let repo_path = PathBuf::from(repo_config.get_storage_path());
        if should_run_tool_scan(&repo_path, repo_name)? {
            println!("  Running tool detection...");
            run_tool_detection(&repo_path, repo_name)?;
        } else {
            println!("  Tool report is up to date, skipping...");
        }
    }

    println!("Repository sync completed!");
    Ok(())
}

fn get_repo_url(repo_config: &config::UserRepoConfig) -> Option<String> {
    // Try to construct URL from source if it looks like a domain
    if repo_config.source.contains('.') {
        Some(format!(
            "https://{}/{}",
            repo_config.source, repo_config.name
        ))
    } else {
        None
    }
}

fn should_run_tool_scan(repo_path: &Path, repo_name: &str) -> Result<bool> {
    let tool_report_path = get_tool_report_path(repo_name);

    // Check if report exists
    if !tool_report_path.exists() {
        println!("    No existing tool report found");
        return Ok(true);
    }

    // Check if report is older than threshold
    let metadata = fs::metadata(&tool_report_path).with_context(|| {
        format!(
            "Failed to read metadata for tool report: {:?}",
            tool_report_path
        )
    })?;

    let modified_time = metadata
        .modified()
        .with_context(|| "Failed to get modification time")?;

    let age = SystemTime::now()
        .duration_since(modified_time)
        .unwrap_or(Duration::from_secs(0));

    let max_age = Duration::from_secs(TOOL_REPORT_MAX_AGE_DAYS * 24 * 60 * 60);

    if age > max_age {
        println!(
            "    Tool report is {} days old, needs refresh",
            age.as_secs() / (24 * 60 * 60)
        );
        Ok(true)
    } else {
        Ok(false)
    }
}

fn run_tool_detection(repo_path: &Path, repo_name: &str) -> Result<()> {
    // Run the detect_tools_command and capture output
    let output = std::process::Command::new(std::env::current_exe()?)
        .arg("detect-tools")
        .arg(repo_path)
        .output()
        .with_context(|| "Failed to execute detect-tools command")?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "Tool detection failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Save the output to a file
    let tool_report_path = get_tool_report_path(repo_name);

    // Ensure the directory exists
    if let Some(parent) = tool_report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {:?}", parent))?;
    }

    fs::write(&tool_report_path, &output.stdout)
        .with_context(|| format!("Failed to write tool report: {:?}", tool_report_path))?;

    println!("    Tool report saved to: {:?}", tool_report_path);
    Ok(())
}

fn get_tool_report_path(repo_name: &str) -> PathBuf {
    config::get_stats_dir()
        .join("tool_reports")
        .join(format!("{}.ndjson", repo_name))
}
