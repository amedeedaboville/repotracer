from dataclasses import dataclass
from typing import Any
import os
import json5


@dataclass()
class RepoConfig(object):
    name: str
    path: str
    # defaults to master if not set
    default_branch: str | None = None


@dataclass()
class StatConfig(object):
    name: str
    description: str
    type: str
    params: Any
    path_in_repo: str


REPO_STORAGE_LOCATION = "repo_storage_location"


def get_default_config():
    return {
        REPO_STORAGE_LOCATION: "./repos",
        "stat_storage": {
            "type": "csv",
            "path": "./stats",  # will store stats in ./stats/<repo_name>/<stat_name>.csv
        },
        "repos": {},
    }


def get_config_path():
    return os.environ.get("REPOTRACER_CONFIG_PATH", "./config.json")


default_config = get_default_config()
config_file_contents = None


def read_config_file():
    global config_file_contents
    if config_file_contents:
        return default_config | (config_file_contents or {})
    # print("Using default config.")
    try:
        print("Looking for config file at", os.path.abspath(get_config_path()))
        with open(get_config_path()) as f:
            config_file_contents = json5.load(f)  # python 3.9 operator for dict update
            return default_config | (config_file_contents or {})
    except FileNotFoundError:
        print(f"Could not find config file at {get_config_path()}. Using defaults.")
        config_file_contents = default_config
        return config_file_contents


def get_repo_storage_location():
    return read_config_file().get(REPO_STORAGE_LOCATION, "./repos")


def get_stat_storage_config():
    return read_config_file()["stat_storage"]


def list_repos():
    try:
        return list(read_config_file()["repos"].keys())
    except KeyError:
        return []


def list_stats_for_repo(repo_name):
    try:
        return list(read_config_file()["repos"][repo_name]["stats"].keys())
    except KeyError:
        return []


def get_config(repo_name, stat_name) -> (RepoConfig, str):
    config_data = read_config_file()
    try:
        repo_subtree = config_data["repos"][repo_name]
        repo_config = RepoConfig(
            name=repo_name,
            path=repo_subtree.get("path"),
            default_branch=repo_subtree.get("default_branch"),
        )
    except KeyError:
        known_repos = ",".join(config_data["repos"].keys())
        raise Exception(
            f"Repo '{repo_name}' not found in config. Known repos are '{known_repos}'"
        )

    try:
        stat_config = repo_subtree["stats"][stat_name]
        stat_config["name"] = stat_name
    except KeyError:
        valid_stats = ", ".join(repo_subtree["stats"].keys())
        raise Exception(
            f"The stat '{stat_name}' does not exist in the config for the repo '{repo_name}'. Here are the known stats: '{valid_stats}'"
        )

    return repo_config, stat_config


def write_config_file(config):
    global config_file_contents
    with open(get_config_path(), "w") as f:
        json5.dump(config, f, indent=4, quote_keys=True)
    config_file_contents = config


def remove_nones(d: dict[Any, Any]):
    return {k: v for k, v in d.items() if v is not None}


def add_repo(repo_config: RepoConfig):
    config = read_config_file()
    config["repos"][repo_config.name] = remove_nones(
        {
            "path": repo_config.path,
            "default_branch": repo_config.default_branch
            if repo_config.default_branch != "master"
            else None,
            "stats": {},
        }
    )
    write_config_file(config)


def add_stat(repo_name: str, stat_config: StatConfig):
    config = read_config_file()
    repo_config = config["repos"][repo_name]
    if "stats" not in repo_config:
        repo_config["stats"] = {}
    repo_config["stats"][stat_config.name] = remove_nones(
        {
            "description": stat_config.description,
            "type": stat_config.type,
            "params": stat_config.params,
            "path_in_repo": stat_config.path_in_repo,
        }
    )

    write_config_file(config)
