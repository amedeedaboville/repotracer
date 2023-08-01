from dataclasses import dataclass
from typing import Any
import os
import json5


def get_config_path():
    return os.environ.get("REPOTRACER_CONFIG_PATH", "./config.json")


def read_config_file():
    # print("Using default config.")
    try:
        with open(get_config_path()) as f:
            config_data = json5.load(f)
            return config_data
    except FileNotFoundError:
        print(f"Could not find config file at {get_config_path()}")
        return get_default_config()


def get_default_config():
    return {
        "repo_storage_location": "./repos",
        "stat_storage": {
            "type": "csv",
            "path": "./stats",  # will store stats in ./stats/<repo_name>/<stat_name>.csv
        },
    }


def get_repo_storage_location():
    return read_config_file()["storage_location"] or "./repos"


def get_stat_storage_config():
    return read_config_file()["stat_storage"]


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
