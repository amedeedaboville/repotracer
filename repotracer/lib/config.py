def get_config(repo_name, stat_name):
    # Let's pretend we open a big config file with a bunch of stats in it
    return {
        "storage_location": "./repos",
        "repos": [
            {
                "name": "svelte",
                "path": "svelte",
                "stats": [
                    {
                        "name": "daily_loc",
                        "description": "The number of ts-ignores in the repo.",
                        "type": "regex",
                        "pattern": "ts-ignore",
                    },
                ],
            }
        ],
    }
