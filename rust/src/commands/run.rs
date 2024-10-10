use chrono::Utc;

use crate::collectors::list_in_range::Granularity;
use crate::config;
use crate::plotter::plot;
use crate::stat::build_measurement;
use crate::storage::write_commit_stats_to_csv;

pub fn run_command(repo: Option<&String>, stat: Option<&String>) {
    match (repo, stat) {
        (Some(repo), Some(stat)) => run_stat(repo, stat),
        _ => println!("Need to specify a repo and a stat"),
    }
}
fn run_stat(repo_name: &str, stat_name: &str) {
    let repo_config = config::get_repo_config(repo_name);
    let repo_path = repo_config
        .storage_path
        .as_ref()
        .expect("Repo doesn't have a storage path, I don't know where to look for it.");
    let stat_config = repo_config
        .stats
        .as_ref()
        .and_then(|s| s.get(stat_name))
        .expect("Didn't find this stat in the config");

    println!("Running {stat_name} on {repo_name} stored at {repo_path}");

    let mut meas = build_measurement(stat_config);
    let mut res = meas
        .run(
            repo_path.to_string(),
            Granularity::Daily,
            (None, None),
            stat_config.path_in_repo.clone(),
        )
        .unwrap();

    let mut plot_df = res.clone();
    write_commit_stats_to_csv(repo_name, stat_name, &mut res).unwrap();
    let stat_description = match stat_name {
        "tokei" => "LOC by Language",
        "grep" => "Number of TODOs",
        "filecount" => "Number of Files",
        _ => stat_config.description.as_ref(),
    };
    println!("plotting");
    plot(
        repo_name,
        stat_name,
        &mut plot_df,
        stat_description,
        &Utc::now(),
    )
    .expect("Error plotting");
}
