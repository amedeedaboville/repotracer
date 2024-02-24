use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use tokei::{CodeStats, Report};

use crate::collectors::cached_walker::CachedWalker;
use crate::collectors::list_in_range::Granularity;
use crate::config;
use crate::plotter::plot;
use crate::stats::common::NumMatches;
use crate::stats::grep::RipgrepCollector;
use crate::stats::tokei::TokeiCollector;
use crate::storage::write_commit_stats_to_csv;

pub fn run_command(repo: Option<&String>, stat: Option<&String>) {
    match (repo, stat) {
        (Some(repo), Some(stat)) => run_stat(repo, stat),
        _ => println!("Need to specify a repo and a stat"),
    }
}
fn run_stat(repo: &str, stat: &str) {
    let repo_config = config::get_repo_config(repo);
    let repo_path = repo_config
        .storage_path
        .as_ref()
        .expect("Repo doesn't have a storage path, I don't know where to look for it.");
    let stat_config = repo_config.stats.as_ref().and_then(|s| s.get(stat));

    println!("Running {stat} on {repo} stored at {repo_path}");
    let pattern = "TODO";

    // let start = NaiveDate::parse_from_str("2020-01-01", "%Y-%m-%d")
    //     .expect("Failed to parse date")
    //     .and_hms(0, 0, 0); // Assuming start of the day
    // let start: DateTime<Utc> = Utc.from_utc_datetime(&start);
    let mut res =
        match stat {
            "tokei" => {
                let file_measurer = Box::new(TokeiCollector::new());
                let mut walker: CachedWalker<CodeStats> =
                    CachedWalker::new(repo_path.to_owned(), file_measurer);
                walker
                    .walk_repo_and_collect_stats(Granularity::Daily, (None, None))
                    .unwrap()
            }
            "grep" => {
                let file_measurer = Box::new(RipgrepCollector::new(pattern));
                let mut walker: CachedWalker<NumMatches> =
                    CachedWalker::new(repo_path.to_owned(), file_measurer);
                walker
                    .walk_repo_and_collect_stats(Granularity::Daily, (None, None))
                    .unwrap()
            }

            _ => panic!("Unknown stat {stat}"),
        };
    let mut plot_df = res.clone();
    let stat_description = match stat {
        "tokei" => "LOC by Language",
        "grep" => "Number of TODOs",
        _ => "Stat results", //todo actually go into the statconfig
    };
    plot(repo, stat, &mut plot_df, &stat_description, &Utc::now()).expect("Error plotting");
    write_commit_stats_to_csv(repo, stat, &mut res).unwrap();

    // let _path_measurer = Box::new(FilePathMeasurer {
    //     callback: Box::new(PathBlobCollector::new(pattern)),
    // });
}
