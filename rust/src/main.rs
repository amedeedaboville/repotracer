use clap::{Arg, Command};
use repotracer::commands::add_stat::add_stat_command;
use repotracer::commands::clone::clone_command;
use repotracer::commands::config::{
    config_add_repo_command, config_location_command, config_show_command,
};
use repotracer::commands::run::run_command;
use repotracer::commands::run_stat::run_stat_command;
use repotracer::commands::serve::serve_command;

fn main() {
    let matches = Command::new("Repotracer")
        .version("0.1")
        .author("Amédée d'Aboville")
        .about("collects stats about repos over time")
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
        Some(("serve", sub_m)) => {
            let port = sub_m.get_one::<String>("port").unwrap();
            serve_command(port);
        }
        _ => unreachable!("Unknown command"),
    }
}
