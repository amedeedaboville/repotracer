from typer.testing import CliRunner

from repotracer.main import app

runner = CliRunner()


def test_help_text_works():
    result = runner.invoke(app, ["--help"])
    assert result.exit_code == 0
    print(result.stdout)
    assert "Usage: " in result.stdout