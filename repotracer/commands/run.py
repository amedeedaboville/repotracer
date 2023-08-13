from repotracer.lib.stat import Stat
from repotracer.lib.config import get_config, list_repos, list_stats_for_repo
from typing import Optional
from typing_extensions import Annotated
import typer
from rich import print
from rich.console import Console
from rich.table import Table
from dataclasses import dataclass

import os


def run(
    repo_name: Annotated[Optional[str], typer.Argument()] = None,
    stat_name: Annotated[Optional[str], typer.Argument()] = None,
    since: Optional[str] = None,
):
    pairs_to_run = []
    match (repo_name, stat_name):
        case (None, None):
            pairs_to_run = [
                (repo, stat)
                for repo in list_repos()
                for stat in list_stats_for_repo(repo)
            ]
        case (repo_name, None):
            pairs_to_run = [
                (repo_name, stat) for stat in list_stats_for_repo(repo_name)
            ]
        case (repo_name, stat_name):
            pairs_to_run = [(repo_name, stat_name)]
        case _:
            logging.error("This combination of repo_name and stat_name is invalid.")
            return
    print_stats_to_run(pairs_to_run)
    for repo, stat in pairs_to_run:
        run_single(repo, stat)


def print_stats_to_run(repo_stats_to_run):
    console = Console()
    table = Table("Repo", "Stat", title="Running the following:", show_lines=True)
    for repo_name, stat_name in repo_stats_to_run:
        table.add_row(f"[green]{repo_name}[/green]", f"[yellow]{stat_name}[/yellow]")
    console.print(table)


def run_single(repo_name: str, stat_name: str):
    print(f"Running [yellow]{stat_name}[/yellow] on [green]{repo_name}[/green]")

    repo_config, stat_params = get_config(repo_name, stat_name)

    stat = Stat(repo_config=repo_config, stat_params=stat_params)
    df = stat.run()
