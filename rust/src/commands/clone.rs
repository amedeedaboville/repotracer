use std::{fs, path::Path, process::Stdio};

use indicatif::{ProgressBar, ProgressStyle};
use url::Url;

pub fn clone_command(clone_urls: Vec<String>) {
    let progress_bar = ProgressBar::new(clone_urls.len() as u64);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {msg}",
            )
            .unwrap(),
    );

    for clone_url in clone_urls.iter() {
        progress_bar.set_message(clone_url.to_string());
        match clone_repo(clone_url) {
            Ok(_) => {}
            Err(e) => {
                progress_bar.println(format!("Failed to clone {}: {}", clone_url, e));
            }
        }
        progress_bar.inc(1); // Increment the progress bar after each attempt
    }

    progress_bar.finish_with_message("Cloning completed");
}

fn clone_repo(clone_url: &str) -> Result<(), String> {
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

    let repo_path = format!("{}/{}/{}", domain, org_name, repo_name);
    if Path::new(&format!("{}/.git", repo_path)).exists() {
        println!("Repository already cloned: {}", repo_path);
        return Ok(()); // Skip cloning since the repo already exists
    }

    fs::create_dir_all(&repo_path).map_err(|_| "Failed to create directories".to_string())?;

    let output = std::process::Command::new("git")
        .arg("clone")
        .arg("--no-checkout")
        .arg("--single-branch")
        .arg(&full_clone_url)
        .arg(format!("{}/{}/{}", domain, org_name, repo_name))
        .stdout(Stdio::inherit()) // This pipes the output to the current process's stdout
        .output()
        .map_err(|_| "Failed to execute git clone".to_string())?;

    if !output.status.success() {
        return Err(format!(
            "Error cloning repository: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    //Sleep 1s in between repos
    std::thread::sleep(std::time::Duration::from_secs(1));
    Ok(())
}
