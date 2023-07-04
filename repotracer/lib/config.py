def get_config(repo_name, stat_name):
    # Let's pretend we open a big config file with a bunch of stats in it
    config_data = {
        "storage_location": "./repos",
        "repos": [
            {
                "name": "svelte",
                "path": "svelte",
                "stats": {
                    "daily_loc": {
                        "name": "daily_loc",
                        "description": "The number of ts-ignores in the repo.",
                        "type": "regex",
                        "pattern": "ts-ignore",
                    },
                },
            }
        ],
    }
    repo_config = config_data["repos"][0]
    stat_config = repo_config["stats"]["daily_loc"]
    return repo_config, stat_config
