import typer
from repotracer.commands import run, add
from repotracer.lib.config import read_config_file


app = typer.Typer(no_args_is_help=True)

app.command()(add.add_repo)
app.command()(add.add_stat)
app.command()(run.run)


@app.command()
def print_config():
    import json
    from repotracer.lib.config import read_config_file

    print(json.dumps(read_config_file(), indent=4))


if __name__ == "__main__":
    read_config_file()
    app()
