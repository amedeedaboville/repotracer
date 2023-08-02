from repotracer.lib.stat import Stat
from repotracer.lib.config import get_config, list_repos, list_stats_for_repo
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
        all_repos = list_repos()
        print(f"Running all stats on all repos: {all_repos}")
        for repo_name in all_repos:
            run_all_on_repo(repo_name)
    if stat_name is None:
        run_all_on_repo(repo_name)


def run_all_on_repo(repo_name: str):
    print(f"Running all stats on repo {repo_name}")
    repo_stat_names = list_stats_for_repo(repo_name)
    print(f"Have {len(repo_stat_names)} stats to run.")
    for stat_name in repo_stat_names:
        run_single(repo_name, stat_name)


def run_single(repo_name: str, stat_name: str):
    print(f"Running stat {stat_name} on repo {repo_name}")

    repo_config, stat_params = get_config(repo_name, stat_name)

    stat = Stat(repo_config=repo_config, stat_params=stat_params)
    df = stat.run()
