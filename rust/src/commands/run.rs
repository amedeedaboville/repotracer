use crate::collectors::cached_walker::CachedWalker;
use crate::collectors::list_in_range::Granularity;
use crate::config::{get_repo_config, RepoConfig};
use crate::stats::common::NumMatches;
use crate::stats::grep::RipgrepCollector;

pub fn run_command(repo: &str, stat: &str) {
    println!("Running {stat} on {repo}");
    let global_config = get_repo_config(repo);
    let repo_config = global_config.repos.get(repo).unwrap();
    let repo_path = repo_config
        .storage_path
        .as_ref()
        .expect("Repo doesn't have a storage path, I don't know where to look for it.");

    let pattern = "TODO";
    let file_measurer = Box::new(RipgrepCollector::new(pattern));
    // let _path_measurer = Box::new(FilePathMeasurer {
    //     callback: Box::new(PathBlobCollector::new(pattern)),
    // });
    // let _tokei_measurer = Box::new(FileContentsMeasurer {
    //     callback: Box::new(TokeiCollector::new()),
    // });
    let mut walker: CachedWalker<NumMatches, NumMatches> = CachedWalker::new(
        repo_path.to_owned(),
        file_measurer,
        // tokei_measurer,
        // Box::new(TokeiReducer {}),
    );
    walker
        .walk_repo_and_collect_stats(Granularity::Daily, (None, None))
        .unwrap();
}
