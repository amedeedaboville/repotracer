use clap::{Arg, Command};
use repotracer::commands::clone::clone_command;
use repotracer::commands::run::run_command;

fn main() {
    let matches = Command::new("Repotracer")
        .version("0.1")
        .author("Amédée d'Aboville")
        .about("collects stats about repos over time")
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
                .about("Clones a list of repositories")
                .arg(
                    Arg::new("clone_urls")
                        .short('c')
                        .long("clone-urls")
                        .value_name("CLONE_URLS")
                        .num_args(1..)
                        .help("Sets the git repository URLs to clone"),
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
            let clone_urls = sub_m
                .get_many::<String>("clone_urls")
                .unwrap_or_default()
                .cloned()
                .collect();
            clone_command(clone_urls);
        }
        _ => unreachable!("Unknown command"),
    }
}
