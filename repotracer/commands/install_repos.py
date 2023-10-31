import os
import shutil
import typer

from collections import namedtuple
from repotracer.commands import run
from repotracer.lib import git, config
from repotracer.lib.measurement import all_measurements, ParamOption
from rich import print
from rich.console import Console
from typing import Optional, List
from typing_extensions import Annotated
import click
import questionary
from enum import Enum

from urllib.parse import urlparse


def install_repos(repos: List[str] = typer.Argument(None)):
    # get the list of repos from the config
    # check to see the installed repos
    # if the repo is not installed, install it
    # todo ideally nobody outside of config should directly call read_config_file

    repos_to_install = repos or config.list_repos()
    for repo_name in repos_to_install:
        repo = config.get_repo_config(repo_name)
        if not os.path.exists(config.get_repo_storage_path(repo_name)):
            git.download_repo(repo.source)
    return
