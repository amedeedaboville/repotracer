use clap::{Arg, Command};
use indicatif::{ProgressBar, ProgressStyle};
use repotracer::collectors::cached_walker::CachedWalker;
use repotracer::stats::common::{FileContentsMeasurer, FilePathMeasurer, NumMatchesReducer};
use repotracer::stats::filecount::PathBlobCollector;
use repotracer::stats::grep::RipgrepCollector;
use repotracer::stats::tokei::TokeiCollector;
use std::fs;
use std::path::Path;
use std::process::Stdio;
use url::Url;

struct Stat {
    name: String,
}
fn main() {
    let matches =
        Command::new("Repotracer")
            .version("0.1")
            .author("Amédée d'Aboville")
            .about("collects stats about repos over time")
            .subcommand(
                Command::new("run")
                    .about("Executes the repository analysis")
                    .arg(
                        Arg::new("repo_path")
                            .short('r')
                            .long("repo")
                            .value_name("REPO_PATH")
                            .default_value("/Users/amedee/clones/betterer")
                            .help("Sets the path to the repo to walk"),
                    )
                    .arg(
                        Arg::new("pattern")
                            .short('p')
                            .long("pattern")
                            .value_name("PATTERN")
                            .default_value("TODO")
                            .help("Sets the pattern to search for"),
                    ),
            )
            .subcommand(
                Command::new("clone")
                    .about("Clones a list of repositories")
                    .arg(
                        Arg::new("clone_urls")
                            .short('c')
                            .long("clone-urls")
                            .value_name("CLONE_URLS")
                            .num_args(1..)
                            .help("Sets the git repository URLs to clone"),
                    ),
            )
            .get_matches();
    match matches.subcommand() {
        Some(("run", sub_m)) => {
            let repo_path = sub_m.get_one::<String>("repo_path").unwrap();
            let pattern = sub_m.get_one::<String>("pattern").unwrap();
            println!(
                "Running analysis on repo: {} with pattern: {}",
                repo_path, pattern
            );
            let file_measurer = Box::new(FileContentsMeasurer {
                callback: Box::new(RipgrepCollector::new(pattern)),
            });
            let _path_measurer = Box::new(FilePathMeasurer {
                callback: Box::new(PathBlobCollector::new(pattern)),
            });
            let _tokei_measurer = Box::new(FileContentsMeasurer {
                callback: Box::new(TokeiCollector::new()),
            });
            let mut walker = CachedWalker::new(
                repo_path.into(),
                file_measurer,
                Box::new(NumMatchesReducer {}),
                // tokei_measurer,
                // Box::new(TokeiReducer {}),
            );
            walker.walk_repo_and_collect_stats().unwrap();
        }
        Some(("clone", sub_m)) => {
            let clone_urls = sub_m
                .get_many::<String>("clone_urls")
                .unwrap_or_default()
                .cloned()
                .collect();
            clone_list_of_repos(clone_urls);
        }
        _ => unreachable!("Unknown command"),
    }
}

fn clone_list_of_repos(clone_urls: Vec<String>) {
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

    Ok(())
}
