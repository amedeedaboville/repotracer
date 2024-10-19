use comfy_table::{Cell, Color, Table};
use dialoguer::console::{style, Term};

use crate::config::{self, GlobalConfig};
use chrono::Utc;

use crate::collectors::list_in_range::Granularity;
use crate::plotter::plot;
use crate::stat::build_measurement;
use crate::storage::write_commit_stats_to_csv;
use crate::util::parse_loose_datetime;

pub fn run_stat_command(
    repo_name: Option<&String>,
    stat_name: Option<&String>,
    since: Option<&String>,
    granularity: Option<&String>,
) {
    let config = config::global_config();
    let pairs_to_run = get_pairs_to_run(&config, repo_name, stat_name);

    print_stats_to_run(&pairs_to_run);

    let granularity = granularity.and_then(|g| Granularity::from_string(g));
    for (repo, stat) in pairs_to_run {
        run_single(&config, &repo, &stat, since, granularity.clone());
    }
}

fn get_pairs_to_run(
    config: &GlobalConfig,
    repo_name: Option<&String>,
    stat_name: Option<&String>,
) -> Vec<(String, String)> {
    match (repo_name, stat_name) {
        (None, None) => config
            .repos
            .iter()
            .flat_map(|(r_name, r_config)| {
                r_config.stats.clone().map_or(vec![], |stats| {
                    stats
                        .keys()
                        .map(|stat| (r_name.clone(), stat.clone()))
                        .collect()
                })
            })
            .collect(),
        (Some(repo), None) => config
            .repos
            .get(repo)
            .and_then(|r| r.stats.as_ref())
            .map_or(vec![], |stats| {
                stats
                    .keys()
                    .map(|stat| (repo.clone(), stat.clone()))
                    .collect()
            }),
        (Some(repo), Some(stat)) => vec![(repo.clone(), stat.clone())],
        _ => {
            eprintln!("This combination of repo_name and stat_name is invalid.");
            vec![]
        }
    }
}

fn print_stats_to_run(pairs_to_run: &[(String, String)]) {
    let mut table = Table::new();
    table
        .load_preset(comfy_table::presets::UTF8_FULL)
        .apply_modifier(comfy_table::modifiers::UTF8_SOLID_INNER_BORDERS)
        .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
        .set_header(vec!["Repo", "Stat"]);

    //I don't really like using the library's color code function
    //bc it's not interoperable, but it breaks without them
    for (repo, stat) in pairs_to_run {
        table.add_row(vec![
            Cell::new(repo).fg(Color::Green),
            Cell::new(stat).fg(Color::Yellow),
        ]);
    }
    println!("{table}");
}

fn run_single(
    config: &GlobalConfig,
    repo_name: &str,
    stat_name: &str,
    since: Option<&String>,
    granularity: Option<Granularity>,
) {
    let term = Term::stdout();
    term.write_line(&format!(
        "Running {} on {}",
        style(stat_name).yellow(),
        style(repo_name).green()
    ))
    .unwrap();

    let repo_config = config
        .repos
        .get(repo_name)
        .expect("Failed to get repo config");
    let stat_config =
        config::get_stat_config(repo_name, stat_name).expect("Failed to get stat config");

    let mut meas = build_measurement(stat_config);
    let mut res = meas
        .run(
            repo_config.get_storage_path(),
            granularity.unwrap_or(Granularity::Daily),
            (since.and_then(|s| parse_loose_datetime(s).ok()), None),
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
