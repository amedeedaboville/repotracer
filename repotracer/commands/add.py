import os
import shutil
import typer

from collections import namedtuple
from repotracer.commands import run
from repotracer.lib import git, config
from rich import print
from rich.console import Console
from typing import Optional, List
from typing_extensions import Annotated
import click
import questionary
from enum import Enum

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
        git.download_repo(url=url_or_path, repo_path=repo_storage_path)
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


qstyle = questionary.Style(
    [
        (
            "highlighted",
            "reverse bold",
        )  # pointed-at choice in select and checkbox prompts
    ]
)


def add_stat(
    repo_name: Annotated[Optional[str], typer.Argument()] = None,
    stat_name: Annotated[Optional[str], typer.Argument()] = None,
):
    if repo_name is None:
        repo_name = questionary.select(
            "Which repo do you want to add a new stat for?",
            choices=config.list_repos() + ["<new repo>"],
            style=qstyle,
            qmark="🕵️",
        ).ask()
        if repo_name == "<new repo>":
            repo_name = questionary.text(
                "What name do you want to give the repo?"
            ).ask()
            add_repo(repo_name)

    if stat_name is None:
        stat_name = questionary.text("What name do you want to give the stat?").ask()
    if stat_name in config.list_stats_for_repo(repo_name):
        print(
            f"The stat '{stat_name}' already exists for the repo '{repo_name}'. Please choose a different name."
        )
        return

    # TODO make this be able to be given from the command line, for now this command
    # will require user input

    stat_type = questionary.select(
        "What type of stat do you want to add?",
        choices=["regex_count", "file_count", "custom_script"],
        style=qstyle,
        # qmark="?"
    ).ask()

    stat_params = promt_build_stat(stat_type)
    print("Building the following stat:")
    print(stat_params)
    stat_config = config.StatConfig(
        name=stat_name,
        description=stat_params.pop("description", None),
        type=stat_type,
        params=stat_params,
        path_in_repo=stat_params.pop("path_in_repo", None),
    )
    config.add_stat(repo_name, stat_config)


ParamOption = namedtuple("ParamOption", ["name", "description", "required"])


def promt_build_stat(stat_type: str):
    common_stat_options = [
        ParamOption(
            name="description",
            description="A description of the stat, used in the title of generated graphs",
            required=False,
        ),
        ParamOption(
            name="start",
            description="The start date for the stat, if you don't want to start at the beginning of the repo",
            required=False,
        ),
        ParamOption(
            name="path_in_repo",
            description="The path in the repo to run the stat on",
            required=False,
        ),
    ]
    # Todo move this into the definition of the measurement/stat type
    params_by_type = {
        "regex_count": [
            ParamOption(
                name="pattern",
                description="The regex pattern to pass to ripgrep",
                required=True,
            ),
            ParamOption(
                name="ripgrep_args",
                description="(Optional) any additional ripgrep args. Useful to exclude files with -g ",
                required=False,
            ),
        ],
        "file_count": ["file_extension"],
        "custom_script": ["script"],
    }
    stat_options = params_by_type[stat_type]
    required_stat_params = [option for option in stat_options if option.required]
    optional_stat_params = [option for option in stat_options if not option.required]
    stat_params = prompt_required_options(required_stat_params)
    print(stat_params)
    stat_params |= prompt_for_options(
        f"The type {stat_type} has the following options. Would you like to set any of these?",
        optional_stat_params,
    )
    print(stat_params)
    common_options = prompt_for_options(
        f"Additionally, stats can have the following options. Would you like to set any of these?",
        common_stat_options,
    )
    print(stat_params, common_options)
    return {**common_options, "params": stat_params}


def prompt_required_options(options: List[ParamOption]):
    choices = {}
    for option in options:
        choices[option.name] = prompt_for_single_param_option(option)
    return choices


def find(list, predicate):
    for item in list:
        if predicate(item):
            return item
    return None


def prompt_for_options(prompt: str, options: List[ParamOption]):
    options_chosen = {}
    options_remaining = {option.name for option in options}
    while options_remaining:
        option_name = questionary.select(
            prompt,
            choices=list(options_remaining) + ["<none>"],
            style=qstyle,
            qmark="📈️",
        ).ask()
        if option_name == "<none>":
            break
        option_value = prompt_for_single_param_option(
            find(options, lambda x: x.name == option_name)
        )
        options_chosen[option_name] = option_value
        options_remaining.remove(option_name)
    return options_chosen


def prompt_for_single_param_option(option: ParamOption):
    return questionary.text(
        f"Please choose a value for '{option.name}': {option.description}."
    ).ask()
