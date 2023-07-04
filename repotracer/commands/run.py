from lib.stats import regex_over_time
import os

run_template = {
    "name": "run",
    "help": "Collect one or more stats",
    "args": [
        {
            "name": "repo_name",
            "help": "Name of the repo to collect stats for",
            "type": "str",
        },
        {
            "name": "stat_name",
            "help": "Name of the stat to collect",
            "type": "str",
        },
        {
            "name": "since",
            "help": "Date to start collecting stats from",
            "type": "str",
        },
    ],
}


def run(repo_name, stat_name, since):
    print(f"Running stat {stat_name} on repo {repo_name} since {since}")
    repo = "svelte"
    config = get_config(repo_name, stat_name)
    stat_fn = regex_over_time
    # cd into repo_path
    os.chdir(repo_path)
    stat_fn(start=since, end=None)
