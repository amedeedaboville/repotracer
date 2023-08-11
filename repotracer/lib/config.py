from dataclasses import dataclass
from typing import Any
import os
import json5


@dataclass()
class RepoConfig(object):
    name: str
    path: str


@dataclass()
class StatConfig(object):
    name: str
    description: str
    type: str
    params: Any
    path_in_repo: str


def get_default_config():
    return {
        "repo_storage_location": "./repos",
        "stat_storage": {
            "type": "csv",
            "path": "./stats",  # will store stats in ./stats/<repo_name>/<stat_name>.csv
        },
    }


def get_config_path():
    return os.environ.get("REPOTRACER_CONFIG_PATH", "./config.json")


config_loaded = False
config = get_default_config()


def read_config_file():
    global config, config_loaded
    if config_loaded:
        return config
    # print("Using default config.")
    try:
        print("Looking for config file at", get_config_path())
        with open(get_config_path()) as f:
            config |= json5.load(f)  # python 3.9 operator for dict update
    except FileNotFoundError:
        print(f"Could not find config file at {get_config_path()}. Keeping defaults.")
    config_loaded = True
    return config


def get_repo_storage_location():
    return read_config_file()["storage_location"] or "./repos"


def get_stat_storage_config():
    return read_config_file()["stat_storage"]


def list_repos():
    try:
        return read_config_file()["repos"].keys()
    except KeyError:
        return []


def list_stats_for_repo(repo_name):
    try:
        return read_config_file()["repos"][repo_name]["stats"].keys()
    except KeyError:
        return []


def get_config(repo_name, stat_name) -> (RepoConfig, str):
    config_data = read_config_file()
    try:
        repo_config = config_data["repos"][repo_name]
        repo_config["name"] = repo_name
    except KeyError:
        known_repos = ", ".join(config_data["repos"].keys())
        raise Exception(
            f"Repo '{repo_name}' not found in config. Known repos are '{known_repos}'"
        )

    try:
        stat_config = repo_config["stats"][stat_name]
        stat_config["name"] = stat_name
    except KeyError:
        valid_stats = ", ".join(repo_config["stats"].keys())
        raise Exception(
            f"The stat '{stat_name}' does not exist in the config for the repo '{repo_name}'. Here are the known stats: '{valid_stats}'"
        )

    return repo_config, stat_config
