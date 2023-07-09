def get_config(repo_name, stat_name):
    # Let's pretend we open a big config file with a bunch of stats in it
    config_data = {
        "storage_location": "./repos",
        "repos": [
            {
                "name": "svelte",
                "path": "svelte",
                "stats": {
                    "count-ts-ignore": {
                        "description": "The number of ts-ignores in the repo.",
                        "type": "regex_count",
                        "pattern": "ts-ignore",
                    },
                },
            }
        ],
    }
    repo_config = next(
        (x for x in config_data["repos"] if x["name"] == repo_name), None
    )
    if not repo_config:
        raise Exception(f"Repo {repo_name} not found in config")

    try:
        stat_config = repo_config["stats"][stat_name]
    except KeyError:
        valid_stats = ", ".join(repo_config["stats"].keys())
        raise Exception(
            f"Stat '{stat_name}' not found in config. Possible values are: '{valid_stats}'"
        )

    return repo_config, stat_config
