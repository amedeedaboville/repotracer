use clap::{Arg, Command};
use repotracer::commands::add_stat::add_stat_command;
use repotracer::commands::clone::clone_command;
use repotracer::commands::config::{
    config_add_repo_command, config_location_command, config_show_command,
};
use repotracer::commands::guess_tools::detect_tools_command;
use repotracer::commands::run::run_command;
use repotracer::commands::run_one_off::run_one_off_command;
use repotracer::commands::run_stat::run_stat_command;
use repotracer::commands::serve::serve_command;
use std::path::PathBuf;

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("GIT_HASH"), ")");
const ABOUT: &str = concat!(
    "repotracer ",
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("GIT_HASH"),
    ")\n",
    env!("CARGO_PKG_DESCRIPTION")
);

fn main() {
    let matches = Command::new("Repotracer")
        .version(VERSION)
        .author("Amédée d'Aboville")
        .about(ABOUT)
        .arg_required_else_help(true)
        .subcommand(
            Command::new("run")
                .about("Executes the given stat on the given repo")
                .arg(
                    Arg::new("repo")
                        .short('r')
                        .long("repo")
                        .value_name("REPO_NAME")
                        .default_value("/Users/amedee/clones/betterer")
                        .help("The short name in the config of the repo, or the full path of the repo"),
                )
                .arg(
                    Arg::new("stat")
                        .short('s')
                        .long("stat")
                        .value_name("STAT_NAME")
                        .default_value("tokei")
                        .help("The name of the stat to run. Leave empty to run all stats on the repo."),
                )
                .arg(
                    Arg::new("pattern")
                        .short('p')
                        .long("pattern")
                        .value_name("PATTERN")
                        .default_value("TODO")
                        .help("Sets the pattern to search for"),
                ),
        )
        .subcommand(
            Command::new("clone")
                .about("Clone one (or more) git repo(s) and add them to the config")
                .arg(
                    Arg::new("clone_urls")
                        .value_name("CLONE_URLS")
                        .num_args(1..)
                        .required(true)
                        .help("The git repository URLs to clone"),
                ),
        )
        .subcommand(
            Command::new("config")
                .about("Manage configuration")
                .arg_required_else_help(true)
                .subcommand(
                    Command::new("location")
                        .about("Print the location of the config file")
                )
                .subcommand(
                    Command::new("show")
                        .about("Print the contents of the config file")
                )
                .subcommand(
                    Command::new("add-repo")
                        .about("Add a repo to the config file")
                        .arg(
                            Arg::new("name")
                                .short('n')
                                .long("name")
                                .value_name("REPO_NAME")
                                .required(true)
                                .help("The name of the repo to add")
                        )
                        .arg(
                            Arg::new("path")
                                .short('p')
                                .long("path")
                                .value_name("REPO_PATH")
                                .required(true)
                                .help("The filesystem path of the repo")
                        )
                )
        )
        .subcommand(
            Command::new("add-stat")
                .about("Add a new stat to a repository")
                .arg(
                    Arg::new("repo")
                        .short('r')
                        .long("repo")
                        .value_name("REPO_NAME")
                        .help("The name of the repository to add the stat to"),
                )
                .arg(
                    Arg::new("stat")
                        .short('s')
                        .long("stat")
                        .value_name("STAT_NAME")
                        .help("The name of the stat to add"),
                ),
        )
        .subcommand(
            Command::new("run-stat")
                .about("Run stats on repositories")
                .arg(
                    Arg::new("repo")
                        .value_name("REPO_NAME")
                        .required(true)
                        .help("The name of the repository to run stats on"),
                )
                .arg(
                    Arg::new("stats")
                        .value_name("STAT_NAMES")
                        .num_args(1..)
                        .help("The name(s) of the stat(s) to run"),
                )
                .arg(
                    Arg::new("granularity")
                        .short('g')
                        .long("granularity")
                        .value_name("GRANULARITY")
                        .default_value("daily")
                        .help("How granular the stats should be: eg daily/weekly/monthly, or every commit."),
                )
                .arg(
                    Arg::new("since")
                        .long("since")
                        .value_name("DATE")
                        .help("Run stats since this date"),
                ),
        )
        .subcommand(
            Command::new("run-one-off")
                .about("Run a one-off stat from a JSON config file")
                .arg(
                    Arg::new("config")
                        .value_name("CONFIG_FILE")
                        .required(true)
                        .help("Path to the JSON file containing the stat configuration"),
                )
                .arg(
                    Arg::new("repo_path")
                        .value_name("REPO_PATH")
                        .required(true)
                        .help("Path to the repository to analyze"),
                )
                .arg(
                    Arg::new("output")
                        .short('o')
                        .long("output")
                        .value_name("OUTPUT_FILE")
                        .help("Path to save the results CSV (defaults to <stat_name>.csv in current directory)"),
                ),
        )
        .subcommand(
            Command::new("serve")
                .about("Start a webserver to interact with Repotracer")
                .arg(
                    Arg::new("port")
                        .short('p')
                        .long("port")
                        .value_name("PORT")
                        .default_value("8080")
                        .help("The port to run the server on"),
                ),
        )
        .subcommand(Command::new("detect-tools")
            .about("Detect tools used in the repo based on indicator files (eg Cargo.toml -> Cargo)")
            .arg(
                Arg::new("tool-definitions")
                    .short('t')
                    .long("tool-definitions")
                    .value_name("TOOL_DEFINITIONS_FILE")
                    .required(true)
                    .help("Path to the tool definitions JSONL file")
            )
            .arg(
                Arg::new("repo-path")
                    .short('r')
                    .long("repo-path")
                    .value_name("REPO_PATH")
                    .help("Path to a single Git repository to scan (ignored if --scan-root is used)")
            )
            .arg(
                Arg::new("scan-root")
                    .long("scan-root")
                    .value_name("SCAN_ROOT")
                    .help("Root directory to scan for repositories (activates multi-repo NDJSON output mode)")
            )
            .arg(
                Arg::new("json")
                    .long("json")
                    .help("Output results as a JSON array for single repository mode (ignored if --scan-root is used)")
            )
            .arg(
                Arg::new("max-depth")
                    .long("max-depth")
                    .value_name("MAX_DEPTH")
                    .default_value("3")
                    .help("Max depth to search for repositories under --scan-root (e.g., 3 for ~/repos/host/org/repo)")
            )
        )
        .get_matches();

    match matches.subcommand() {
        Some(("run", sub_m)) => {
            let repo_path = sub_m.get_one::<String>("repo");
            let stat_name = sub_m.get_one::<String>("stat");
            run_command(repo_path, stat_name);
        }
        Some(("clone", sub_m)) => {
            let clone_urls: Vec<String> = sub_m
                .get_many::<String>("clone_urls")
                .unwrap_or_default()
                .cloned()
                .collect();
            clone_command(clone_urls);
        }
        Some(("add-stat", add_stat_m)) => {
            let repo_name = add_stat_m.get_one::<String>("repo");
            let stat_name = add_stat_m.get_one::<String>("stat");
            add_stat_command(repo_name, stat_name);
        }
        Some(("config", sub_m)) => match sub_m.subcommand() {
            Some(("location", _)) => config_location_command(),
            Some(("show", _)) => config_show_command().unwrap(),
            Some(("add-repo", add_repo_m)) => {
                let name = add_repo_m.get_one::<String>("name").unwrap();
                let path = add_repo_m.get_one::<String>("path").unwrap();
                let _ = config_add_repo_command(name, path);
            }
            _ => unreachable!("Unknown config subcommand"),
        },
        Some(("run-stat", sub_m)) => {
            let repo = sub_m.get_one::<String>("repo").unwrap();
            let stats: Vec<String> = sub_m
                .get_many::<String>("stats")
                .unwrap_or_default()
                .cloned()
                .collect();
            run_stat_command(
                Some(repo),
                &stats,
                sub_m.get_one::<String>("since"),
                sub_m.get_one::<String>("granularity"),
            );
        }
        Some(("run-one-off", sub_m)) => {
            let config_path = sub_m.get_one::<String>("config").unwrap();
            let repo_path = sub_m.get_one::<String>("repo_path").unwrap();
            let output = sub_m.get_one::<String>("output");
            run_one_off_command(config_path, repo_path, output);
        }
        Some(("serve", sub_m)) => {
            let port = sub_m.get_one::<String>("port").unwrap();
            serve_command(port);
        }
        Some(("detect-tools", sub_m)) => {
            let tool_definitions = sub_m.get_one::<String>("tool-definitions").unwrap();
            let repo_path = sub_m.get_one::<String>("repo-path");
            let json = sub_m.contains_id("json");
            let scan_root = sub_m.get_one::<String>("scan-root");
            let max_depth = sub_m
                .get_one::<String>("max-depth")
                .unwrap()
                .parse::<usize>()
                .unwrap();

            detect_tools_command(
                &PathBuf::from(tool_definitions),
                repo_path.map(|p| PathBuf::from(p)),
                json,
                scan_root.map(|p| PathBuf::from(p)),
                max_depth,
            )
            .unwrap();
        }
        _ => unreachable!("Unknown command"),
    }
}
