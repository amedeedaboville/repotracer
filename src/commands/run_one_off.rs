use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

use crate::collectors::list_in_range::Granularity;
use crate::config::UserStatConfig;
use crate::stat::build_measurement;
use crate::storage::write_commit_stats_to_csv;

pub fn run_one_off_command(config_path: &str, repo_path: &str, output_path: Option<&String>) {
    let mut file = File::open(config_path).expect("Failed to open config file");
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .expect("Failed to read config file");
    let stat_config: UserStatConfig =
        serde_json::from_str(&contents).expect("Failed to parse config file as UserStatConfig");

    println!(
        "Running {} stat on repository at {}",
        stat_config.name, repo_path
    );

    let mut meas = build_measurement(&stat_config);
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
        .expect("Failed to run measurement");

    let output_path = PathBuf::from(
        output_path
            .map(|p| p.to_string())
            .unwrap_or_else(|| format!("{}.csv", stat_config.name)),
    );

    write_commit_stats_to_csv(
        "custom_repo".into(),
        &stat_config.name,
        &mut res,
        Some(PathBuf::from(&output_path)),
    )
    .expect("Failed to write results to CSV");

    println!("Results written to {:?}", output_path);
}
