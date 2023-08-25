import os
import shutil
import typer

from repotracer.commands import run
from repotracer.lib import git, config
from rich import print
from rich.console import Console
from typing import Optional
from typing_extensions import Annotated
from urllib.parse import urlparse


def is_url(x):
    try:
        result = urlparse(x)
        return all([result.scheme, result.netloc])
    except:
        return False


def add_repo(url_or_path: str, name: Optional[str] = None):
    if is_url(url_or_path):
        repo_name = name or os.path.splitext(os.path.basename(url_or_path))[0]
        repo_storage_path = os.path.join("./repos", repo_name)
        it.download_repo(url=url_or_path, repo_path=repo_storage_path)
    else:
        repo_name = name or os.path.basename(url_or_path)
        repo_storage_path = os.path.join("./repos", repo_name)
        os.makedirs(repo_storage_path, exist_ok=True)
        shutil.copytree(url_or_path, repo_storage_path)

    config.add_repo(
        repo_name=repo_name,
        repo_path=repo_storage_path,
    )
    # optionally ask the user if they want to add any stats for this repo
    # and call add_stat() if they do


def add_stat(
    repo_name: Annotated[
        str, typer.Option(prompt="What repo do you want to add a stat for?")
    ],
    stat_name: Annotated[
        str, typer.Option(prompt="What name do you want to give your stat?")
    ] = None,
    stat_type: Optional[str] = None,
):
    if repo_name is None:
        if repo_name not in config.list_repos():
            should_add = typer.confirm(
                f"The repo {repo_name} was not found in the config. Would you like to add it now?"
            )
            if should_add:
                add_repo(repo_name)
            else:
                return

    if stat_type is None:
        stat_type = typer.prompt(
            "What kind of stat do you want to add? (built-in or custom)"
        )
    # show them the list of built-in stats, or they can put in a custom one
    # If there are any parameters for the stat, ask the user for them
    # store this info in the config
    # ask the user if they want to run the stat now
    pass
