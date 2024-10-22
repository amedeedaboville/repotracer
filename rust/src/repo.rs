use crate::config;
use std::{fs, path::Path, process::Stdio};

use url::Url;

// Equivalent of  basename $(git symbolic-ref --short refs/remotes/origin/HEAD)
pub fn get_default_branch() -> Result<String, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("git")
        .args(["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .output()?;

    if !output.status.success() {
        return Err("Failed to get default branch".into());
    }

    let branch = String::from_utf8(output.stdout)?;
    Ok(std::path::Path::new(&branch.trim())
        .file_name()
        .and_then(|s| s.to_str())
        .map(String::from)
        .ok_or("Invalid branch name")?)
}

fn parse_clone_url(clone_url: &str) -> Result<(String, String, String, String), String> {
    let (full_clone_url, domain, org_name, repo_name) = if clone_url.contains('/')
        && !clone_url.starts_with("http")
    {
        let domain = "github.com";
        let full_clone_url = format!("https://{}/{}.git", domain, clone_url);
        let parts: Vec<&str> = clone_url.split('/').collect();
        if parts.len() == 2 {
            (
                full_clone_url,
                domain.to_string(),
                parts[0].to_string(),
                parts[1].to_string(),
            )
        } else {
            return Err("Invalid clone URL format. Expected format: $some-org/$some-repo or a full URL like https://github.com/postgres/postgres.git.".to_string());
        }
    } else {
        match Url::parse(clone_url) {
            Ok(parsed_url) => {
                let domain = parsed_url.domain().unwrap_or("unknown").to_string();
                let path_segments: Vec<&str> = parsed_url
                    .path_segments()
                    .map_or_else(Vec::new, |c| c.collect());
                if path_segments.len() >= 2 {
                    let repo_name = path_segments
                        .last()
                        .unwrap()
                        .trim_end_matches(".git")
                        .to_string();
                    (
                        clone_url.to_string(),
                        domain,
                        path_segments[0].to_string(),
                        repo_name,
                    )
                } else {
                    return Err("Invalid clone URL format. Expected at least an organization and a repository name in the path.".to_string());
                }
            }
            Err(_) => {
                return Err("Failed to parse the clone URL.".to_string());
            }
        }
    };
    Ok((full_clone_url, domain, org_name, repo_name))
}

pub fn clone_repo(clone_url: &str) -> Result<(), String> {
    let (full_clone_url, domain, org_name, repo_name) = parse_clone_url(clone_url)?;
    let repo_path = format!("{}/{}/{}", domain, org_name, repo_name);
    if config::get_repo_config(&repo_name).is_some() {
        println!("Repository {repo_name} already exists in the config file, checking that it's cloned : {}", repo_name);
        ensure_cloned(&full_clone_url, &repo_path)
    } else {
        let repo_config = config::UserRepoConfig {
            name: repo_name,
            storage_path: Some(repo_path),
            source: domain,
            default_branch: get_default_branch().ok(),
            stats: None,
        };
        config::add_repo(repo_config);
        Ok(())
    }
}
fn ensure_cloned(full_clone_url: &str, repo_path: &str) -> Result<(), String> {
    if Path::new(&format!("{}/.git", repo_path)).exists() {
        println!("Repository already cloned, refreshing: {}", repo_path);
        //refresh the repo with git pull
        let _output = std::process::Command::new("git")
            .arg("pull")
            .arg("--rebase")
            .arg(repo_path)
            .output()
            .map_err(|_| "Failed to execute git pull".to_string())?;

        return Ok(()); // Skip cloning since the repo already exists
    } else {
        fs::create_dir_all(repo_path).map_err(|_| "Failed to create directories".to_string())?;

        let output = std::process::Command::new("git")
            .arg("clone")
            .arg("--no-checkout")
            .arg("--single-branch")
            .arg(full_clone_url)
            .arg(repo_path)
            .stdout(Stdio::inherit()) // This pipes the output to the current process's stdout
            .output()
            .map_err(|_| "Failed to execute git clone".to_string())?;

        if !output.status.success() {
            return Err(format!(
                "Error cloning repository: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    Ok(())
}
