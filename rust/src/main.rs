use clap::{Arg, Command};
use repotracer::collectors::cached_walker::{CachedWalker, FileContentsMeasurer, FilePathMeasurer};
use repotracer::stats::common::NumMatchesReducer;
use repotracer::stats::filecount::PathBlobCollector;
use repotracer::stats::grep::RipgrepCollector;

struct Stat {
    name: String,
}
fn main() {
    let matches = Command::new("Repotracer")
        .version("0.1")
        .author("Amédée d'Aboville")
        .about("collects")
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
        )
        .get_matches();
    let repo_path = matches.get_one::<String>("repo_path").unwrap();
    let pattern = matches.get_one::<String>("pattern").unwrap();
    // let mut walker = CachedWalker::new(
    //     repo_path,
    //     Box::new(FileContentsMeasurer {
    //         callback: Box::new(RipgrepCollector::new(pattern)),
    //     }),
    //     Box::new(NumMatchesReducer {}),
    // );
    // walker.walk_repo_and_collect_stats();
    let mut walker = CachedWalker::new(
        repo_path,
        Box::new(FilePathMeasurer {
            callback: Box::new(PathBlobCollector::new(pattern)),
        }),
        Box::new(NumMatchesReducer {}),
    );
    walker.walk_repo_and_collect_stats();
    // // let mut walker = TreeWalker::new(
    // //     repo_path,
    // Box::new(PathBlobCollector::new("*test*")),
    // //     Box::new(NumMatchesReducer {}),
    // // );
    // walker.walk_repo_and_collect_stats();
}
