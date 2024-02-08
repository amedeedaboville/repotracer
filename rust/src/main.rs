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
    let file_measurer = Box::new(FileContentsMeasurer {
        callback: Box::new(RipgrepCollector::new(pattern)),
    });
    let path_measurer = Box::new(FilePathMeasurer {
        callback: Box::new(PathBlobCollector::new(pattern)),
    });
    let mut walker = CachedWalker::new(
        repo_path.into(),
        // file_measurer,
        file_measurer,
        Box::new(NumMatchesReducer {}),
    );
    walker.walk_repo_and_collect_stats(true);
}
