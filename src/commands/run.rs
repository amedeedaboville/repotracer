use crate::collectors::list_in_range::Granularity;
use crate::config;
use crate::stat::build_measurement;
use crate::storage::write_commit_stats_to_csv;

pub fn run_command(repo: Option<&String>, stat: Option<&String>) {
    match (repo, stat) {
        (Some(repo), Some(stat)) => run_stat(repo, stat),
        _ => println!("Need to specify a repo and a stat"),
    }
}
fn run_stat(repo_name: &str, stat_name: &str) {
    let repo_config = config::get_repo_config(repo_name).expect("Expected repo to exist in config");
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
            stat_config
                .granularity
                .clone()
                .unwrap_or(Granularity::Daily),
            (None, None),
            stat_config.path_in_repo.clone(),
        )
        .unwrap();

    write_commit_stats_to_csv(repo_name, stat_name, &mut res, None).unwrap();
}
