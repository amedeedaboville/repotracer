from lib.stats import regex_stat
from lib.config import get_config
from typing import Optional
from typing_extensions import Annotated
import typer

import os


def run(
    repo_name: Optional[str] = None,
    stat_name: Annotated[Optional[str], typer.Argument()] = None,
    since: Optional[str] = None,
):
    print(repo_name, stat_name, since)
    if repo_name is None:
        # todo, print Running x stats on y repos
        # and run the stats grouped by repo
        print("Running all stats on all repos")
    if repo_name is not None and stat_name is not None:
        print(f"Running stat {stat_name} on repo {repo_name}")
        run_single(repo_name, stat_name, since)


def run_all_on_repo(repo_name: str):
    print(f"Running all stats on repo {repo_name}")
    all_stats_for_repo = get_config(repo_name)["stats"]
    print(f"Have {len(all_stats_for_repo)} stats to run.")
    for stat_name in all_stats_for_repo.keys():
        run_single(repo_name, stat_name)


def run_single(repo_name: str, stat_name: str):
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
