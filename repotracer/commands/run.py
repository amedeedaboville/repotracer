from lib.stats import regex_stat
from lib.config import get_config
import os


def define_command(run_parser):
    run_parser.add_argument(
        "repo_name", help="Name of the repo to collect stats for", type=str
    )
    run_parser.add_argument("stat_name", help="Name of the stat to collect", type=str)
    run_parser.add_argument(
        "since", help="Date to start collecting stats from", type=str
    )
    run_parser.set_defaults(func=run)


def run(args):
    repo_name = args.repo_name
    stat_name = args.stat_name
    since = args.since
    print(f"Running stat {stat_name} on repo {repo_name} since {since}")
    repo = "svelte"
    repo_config, stat_config = get_config(repo_name, stat_name)
    print(repo_config, stat_config)
    print(os.getcwd())
    # cd into repo_path
    os.chdir("repos/" + repo_config["path"])
    # todo pass the args to the stat
    df = regex_stat("ts-ignore")()
    df.to_csv("output.csv", date_format="%Y-%m-%d")
