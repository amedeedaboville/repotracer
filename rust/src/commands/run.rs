use tokei::{CodeStats, Report};

use crate::collectors::cached_walker::CachedWalker;
use crate::collectors::list_in_range::Granularity;
use crate::config;
use crate::stats::common::NumMatches;
use crate::stats::grep::RipgrepCollector;
use crate::stats::tokei::TokeiCollector;

pub fn run_command(repo: Option<&String>, stat: Option<&String>) {
    match (repo, stat) {
        (Some(repo), Some(stat)) => run_stat(repo, stat),
        _ => println!("Need to specify a repo and a stat"),
    }
}
fn run_stat(repo: &str, stat: &str) {
    println!("Running {stat} on {repo}");
    let repo_config = config::get_repo_config(repo);
    let repo_path = repo_config
        .storage_path
        .as_ref()
        .expect("Repo doesn't have a storage path, I don't know where to look for it.");

    let pattern = "TODO";
    // let file_measurer = Box::new(RipgrepCollector::new(pattern));
    // let _path_measurer = Box::new(FilePathMeasurer {
    //     callback: Box::new(PathBlobCollector::new(pattern)),
    // });
    let file_measurer = Box::new(TokeiCollector::new());
    let mut walker: CachedWalker<Report, CodeStats> = CachedWalker::new(
        repo_path.to_owned(),
        file_measurer,
        // tokei_measurer,
        // Box::new(TokeiReducer {}),
    );
    walker
        .walk_repo_and_collect_stats(Granularity::Daily, (None, None))
        .unwrap();
}
