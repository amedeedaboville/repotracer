use std::collections::HashMap;
use std::fmt::{Debug, Display};

use clap::{Arg, Command};
use repotracer::cached_walker::CachedWalker;
use repotracer::stats::common::NumMatchesReducer;
use repotracer::stats::grep::RipgrepCollector;

struct Stat {
    name: String,
}
fn main() {
    let matches =
        Command::new("Repotracer")
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
            .get_matches();
    let repo_path = matches.get_one::<String>("repo_path").unwrap();
    let mut walker = CachedWalker::new(
        repo_path,
        Box::new(RipgrepCollector::new("TODO")),
        Box::new(NumMatchesReducer {}),
    );
    walker.walk_repo_and_collect_stats();
}
