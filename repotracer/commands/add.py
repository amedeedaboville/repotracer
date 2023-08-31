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


def is_url(x):
    try:
        result = urlparse(x)
        return all([result.scheme, result.netloc])
    except:
        return False


def add_repo(url_or_path: str, name: Optional[str] = None):
    console = Console()
    if is_url(url_or_path):
        repo_config = git.download_repo(url=url_or_path)
    else:
        # todo make a more full featured version of this in the git module
        source_path = os.path.expanduser(url_or_path)
        repo_name = name or os.path.basename(source_path)
        repo_storage_path = os.path.join("./repos", repo_name)
        repo_git_dir = os.path.join(repo_storage_path, ".git")
        cwd = os.getcwd()
        os.makedirs(repo_git_dir, exist_ok=True)
        git_dir = os.path.join(source_path, ".git")
        with console.status(f"Copying {git_dir} into '{repo_storage_path}'..."):
            shutil.copytree(git_dir, repo_git_dir, dirs_exist_ok=True)
        os.chdir(repo_storage_path)
        git.checkout(".")
        default_branch = git.get_default_branch()
        git.checkout(default_branch)
        os.chdir(cwd)
        repo_config = config.RepoConfig(
            name=repo_name, path=repo_name, default_branch=default_branch
        )

    config.add_repo(repo_config=repo_config)
    return repo_config
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
            url_or_path = questionary.text(
                "Enter either a repo URL or paste a path to a local repo on your machine (we will make copy it into the repos folder):"
            ).ask()
            added_config = add_repo(url_or_path)
            repo_name = added_config.name

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

    stat_description = questionary.text(
        "Give your stat a description, to explain what it does (will be used as the title of generated graphs):",
    ).ask()

    stat_params = promt_build_stat(stat_type)
    print("Building the following stat:")
    print(stat_params)
    stat_config = config.StatConfig(
        name=stat_name,
        description=stat_description,
        type=stat_type,
        params=stat_params.pop("params", None),
        path_in_repo=stat_params.pop("path_in_repo", None),
    )
    config.add_stat(repo_name, stat_config)
    run_now = questionary.confirm("Do you want to run this stat now?").ask()
    if run_now:
        run.run(repo_name, stat_name)


def promt_build_stat(stat_type: str):
    common_stat_options = [
        # Todo the required ones like type and description should just be done in the other function, or moved into here
        # ParamOption(
        #     name="description",
        #     description="A description of the stat, used in the title of generated graphs",
        #     required=True,
        # ),
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
    stat_options = all_measurements[stat_type].params
    required_stat_params = [option for option in stat_options if option.required]
    optional_stat_params = [option for option in stat_options if not option.required]
    stat_params = prompt_required_options(required_stat_params)
    stat_params |= prompt_for_options(
        f"The type {stat_type} has the following options. Would you like to set any of these?",
        optional_stat_params,
    )
    common_options = prompt_for_options(
        f"Additionally, stats can have the following options. Would you like to set any of these?",
        common_stat_options,
    )
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
