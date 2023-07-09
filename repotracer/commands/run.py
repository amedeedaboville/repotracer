from lib.stats import regex_stat
from lib.config import get_config
from typing import Optional

import os


def define_command(run_parser):
    run_parser.add_argument(
        "repo_name", help="Name of the repo to collect stats for", type=str
    )
    run_parser.add_argument(
        "stat_name", help="Name of the stat to collect", type=str, default="_all"
    )
    run_parser.add_argument(
        "since", help="Date to start collecting stats from", type=str
    )
    run_parser.set_defaults(func=run)


def run(args):
    repo_name, stat_name, since = args.repo_name, args.stat_name, args.since
    print(f"Running stat {stat_name} on repo {repo_name} since {since}")

    repo_config, stat_params = get_config(repo_name, stat_name)

    # todo set the /repos path in the config
    os.chdir("repos/" + repo_config["path"])
    print(stat_params)

    # todo pick the stat function based on the type
    # todo pass the whole stat_params to the stat function
    # so it can be polymorphic
    df = regex_stat(stat_params["pattern"])()
    df.to_csv("output.csv", date_format="%Y-%m-%d")
