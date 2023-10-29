from dataclasses import dataclass, asdict
from typing import Any, Optional
import dacite
import os
import json5

# By default, repotracer will store 3 kinds of data inside its root directory:
# 1. Repos, under ROOT_DIR/repos. You can change this with the repo_storage_location config key.
# 2. The config file, under ROOT_DIR/config.json.
# 3. The stats, under ROOT_DIR/stats/<repo_name>/<stat_name>.{csv,png}
# Possibly later these will be configurable differently.

ROOT_DIR = os.environ.get(
    "REPOTRACER_ROOT_DIR",
    os.environ.get(
        os.path.join("XDG_CONFIG_HOME", "repotracer"),
        os.path.expanduser("~/.repotracer"),
    ),
)


@dataclass()
class StatConfig(object):
    name: str
    description: str
    type: str
    params: Any
    path_in_repo: Optional[str]
    start: Optional[str]


@dataclass()
class RepoConfig(object):
    name: Optional[str]
    path: Optional[str]
    # defaults to master if not set
    default_branch: str | None = None
    stats: dict[str, StatConfig] | None = None


@dataclass()
class GlobalConfig(object):
    repo_storage_location: Optional[str]
    stat_storage: Optional[dict[str, Any]]
    repos: dict[str, RepoConfig]


# A dict version of default_config:
default_config_dict = {
    "stat_storage": {
        "type": "csv",
    },
    "repos": {},
}
# a dataclass version of default_config:
default_config = dacite.from_dict(GlobalConfig, default_config_dict)


def get_config_path():
    return os.path.join(ROOT_DIR, "config.json")

config_file_contents = None


def read_config_file() -> GlobalConfig:
    global config_file_contents
    if config_file_contents is None:
        try:
            print("Looking for config file at", os.path.abspath(get_config_path()))
            with open(get_config_path()) as f:
                config_file_contents = json5.load(
                    f
                )  # python 3.9 operator for dict update
        except FileNotFoundError:
            print(f"Could not find config file at {get_config_path()}, writing a default config file there.")
            write_config_file(default_config)
            config_file_contents = default_config_dict
    combined_config = default_config_dict | config_file_contents
    apply_implicit_names(combined_config) # mutates in place
    return dacite.from_dict(GlobalConfig, combined_config)

def apply_implicit_names(config_dict: dict[str, Any]):
    for repo in config_dict.get("repos", {}).values():
        for name, stat_obj in (repo.get("stats") or  {}).items():
            if "name" not in stat_obj:
                stat_obj["name"] = name


def get_repos_dir():
    return read_config_file().repo_storage_location or os.path.join(ROOT_DIR, "repos")


def get_stats_dir():
    return read_config_file().stat_storage.get("path") or os.path.join(ROOT_DIR, "stats")


def list_repos():
    try:
        return list(read_config_file().repos.keys())
    except KeyError:
        return []


def list_stats_for_repo(repo_name):
    try:
        return list((read_config_file().repos[repo_name].stats or {}).keys())
    except KeyError:
        return []


def get_config(repo_name, stat_name) -> (RepoConfig, str):
    config_data = read_config_file()
    try:
        repo_subtree : RepoConfig = config_data.repos[repo_name]
        repo_config = RepoConfig(
            name=repo_name,
            path=repo_subtree.path,
            default_branch=repo_subtree.default_branch,
        )
    except KeyError:
        known_repos = ",".join(config_data.repos.keys())
        raise Exception(
            f"Repo '{repo_name}' not found in config. Known repos are '{known_repos}'"
        )

    try:
        stat_config = repo_subtree.stats[stat_name]
        stat_config.name = stat_name
    except KeyError:
        valid_stats = ", ".join(repo_subtree.stats.keys())
        raise Exception(
            f"The stat '{stat_name}' does not exist in the config for the repo '{repo_name}'. Here are the known stats: '{valid_stats}'"
        )

    return repo_config, stat_config


def write_config_file(config):
    global config_file_contents
    config_path = get_config_path()
    config_dir = os.path.dirname(config_path)
    if not os.path.exists(config_dir):
        os.makedirs(config_dir, exist_ok=True)
    #Don't write None's into the config file
    to_write = asdict(config, dict_factory=lambda x: {k: v for (k, v) in x if v is not None})
    with open(config_path, "w") as f:
        json5.dump(to_write, f, indent=4, quote_keys=True)
    config_file_contents = to_write


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
    repo_config = config.repos[repo_name]
    if not repo_config.stats:
        repo_config.stats = {}
    repo_config.stats[stat_config.name] = stat_config
    # remove_nones(
    #     {
    #         "description": stat_config.description,
    #         "type": stat_config.type,
    #         "params": stat_config.params,
    #         "path_in_repo": stat_config.path_in_repo,
    #     }
    # )

    write_config_file(config)
