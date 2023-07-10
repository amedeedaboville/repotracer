import typer
import commands.run

app = typer.Typer(no_args_is_help=True)
app.command()(commands.run.run)


@app.command()
def print_config():
    import json
    from lib.config import read_config_file

    print(json.dumps(read_config_file(), indent=4))


@app.command()
def add_repo():
    # ask the user for the repo URL
    # ask the user for the repo name if it's different
    # add the repo to the config
    # optionally ask the user if they want to add any stats for this repo
    # and call add_stat() if they do
    pass


@app.command()
def add_stat():
    # if the user didn't specify a repo, ask them for one, or create it with add_repo()
    # ask the user for what kind of stat they want to add
    # show them the list of built-in stats, or they can put in a custom one
    # If there are any parameters for the stat, ask the user for them
    # ask the user for the name of the stat
    # store this info in the config
    # ask the user if they want to run the stat now
    pass


@app.command()
def plot(repo: str, stat: str):
    # Open the storage location for the stat
    # Plot the data
    pass


if __name__ == "__main__":
    app()
