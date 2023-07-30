from lib.stat import Stat
from lib.config import get_config
from typing import Optional
from typing_extensions import Annotated
import typer

import os


def run(
    repo_name: Annotated[Optional[str], typer.Argument()] = None,
    stat_name: Annotated[Optional[str], typer.Argument()] = None,
    since: Optional[str] = None,
):
    print(repo_name, stat_name, since)
    if repo_name is None:
        # todo, print Running x stats on y repos
        # and run the stats grouped by repo
        print("Running all stats on all repos")
    if repo_name is not None and stat_name is not None:
        run_single(repo_name, stat_name)


def run_all_on_repo(repo_name: str):
    print(f"Running all stats on repo {repo_name}")
    all_stats_for_repo = get_config(repo_name)["stats"]
    print(f"Have {len(all_stats_for_repo)} stats to run.")
    for stat_name in all_stats_for_repo.keys():
        run_single(repo_name, stat_name)


def run_single(repo_name: str, stat_name: str):
    print(f"Running stat {stat_name} on repo {repo_name}")

    repo_config, stat_params = get_config(repo_name, stat_name)

    stat = Stat(repo_config=repo_config, stat_params=stat_params)
    df = stat.run()
