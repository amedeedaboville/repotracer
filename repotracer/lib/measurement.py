from . import git
from collections import namedtuple
from dataclasses import dataclass
from typing import Callable, Generic, TypeVar, Type, Any, TypedDict
from typing_extensions import Protocol
import json
import subprocess

##############################################################################
## Actual measurement functions
##############################################################################


def script(cmd):
    return lambda: subprocess.check_output(cmd, shell=True)


def script_now(cmd):
    try:
        return subprocess.check_output(cmd, shell=True).decode("utf-8")
    except subprocess.CalledProcessError as e:
        # ignoGenericre exit code 1
        if e.returncode == 1:
            return e.output
        print("Error in script_now")
        print(e)
        print(e.output)
        raise e


def float_or_int(s):
    try:
        return int(s)
    except ValueError:
        try:
            return float(s)
        except Exception as e:
            return float("NaN")


def script_auto(cmd, return_type):
    output = script_now(cmd)
    if return_type == "number":
        return {"output": float_or_int(output)}
    elif return_type == "json":
        return json.loads(output)
    else:
        raise ValueError(f"Unknown return type {return_type}")


def ripgrep_count_file(pattern):
    return int(script(f"rg -l {pattern} | wc -l"))


def rg_count(pattern: str, rg_args: str) -> int:
    filenames_with_counts = script_now(f"rg '{pattern}' --count {rg_args or ''}")
    res = {
        "total": sum(
            int(line.split(":")[-1]) for line in filenames_with_counts.splitlines()
        )
    }
    return res


def fd_count(pattern: str, extra_cli_args: str) -> int:
    filenames_with_counts = script_now(
        f"fd --glob '{pattern}' --type file {extra_cli_args or ''}"
    )
    # todo: eventually, store the individual file names as intermediate results
    res = {"total": len(filenames_with_counts.splitlines())}
    return res


def tokei_count(languages: str, extra_cli_args: str) -> int:
    if languages == "all":
        languages = ""
    language_arg = f"-t {languages}" if languages else ""
    json_out = script_now(f"tokei --output json {language_arg} {extra_cli_args or ''}")
    parsed = json.loads(json_out)
    code_totals = {language: data["code"] for language, data in parsed.items()}
    breakdown_languages = True
    if breakdown_languages:
        del code_totals["Total"]
        return code_totals
    else:
        return code_totals["Total"]


def jsx_to_tsx():
    return script_now("tokei --output json --languages jsx,tsx,js,ts")


##############################################################################
# Measurement Configs
# and the dictionary of all measurements
##############################################################################


class MeasurementConfig(object):
    pass


TConfig = TypeVar("TConfig", bound=MeasurementConfig)


class Measurement(Protocol):
    def __call__(self, Tconfig) -> list[Any]:
        pass


class FunctionMeasurement(Generic[TConfig]):
    """A Measurement that consists of a single function.
    Easy to turn a function into a measurement."""

    def __init__(self, fn: Callable[[TConfig], list[Any]]):
        self.fn = fn

    def __call__(self, config: TConfig):
        def wrapper() -> list[Any]:
            return self.fn(config)

        return wrapper


class RegexConfig(TypedDict):
    pattern: str


class FileCountConfig(TypedDict):
    pattern: str


class LOCCountConfig(TypedDict):
    pattern: str


class ScriptConfig(TypedDict):
    command: str
    return_type: str


RegexMeasurement = FunctionMeasurement[RegexConfig](
    lambda config: rg_count(config["pattern"], config.get("ripgrep_args"))
)

FileCountMeasurement = FunctionMeasurement[FileCountConfig](
    lambda config: fd_count(config["pattern"], config.get("fd_args"))
)

LocCountMeasurement = FunctionMeasurement[LOCCountConfig](
    lambda config: tokei_count(config.get("languages"), config.get("total"))
)

ScriptMeasurement = FunctionMeasurement[ScriptConfig](
    lambda config: script_auto(config["command"], config.get("return_type", "number"))
)
# jsx_to_tsx = FunctionMeasurement(tokei_specific(["jsx", "tsx", "js", "ts"]))
# authors_per_month = FunctionMeasurement(git.get_commit_author)

ParamOption = namedtuple("ParamOption", ["name", "description", "required"])
MeasurementDef = namedtuple("MeasurementDef", ["obj", "params"])

# todo ideally we could remove the duplication between the TypedDict subclasses (eg FileCountConfig)
# and these ParamOption lists, which both store the same information about which parameters a measurment takes.
all_measurements = {
    "regex_count": MeasurementDef(
        obj=RegexMeasurement,
        params=[
            ParamOption(
                name="pattern",
                description="The regex pattern to pass to ripgrep",
                required=True,
            ),
            ParamOption(
                name="ripgrep_args",
                description="(Optional) any additional ripgrep args. Useful to exclude files with -g ",
                required=False,
            ),
        ],
    ),
    "file_count": MeasurementDef(
        obj=FileCountMeasurement,
        params=[
            ParamOption(
                name="pattern",
                description="""The glob pattern to pass to fd. eg: '*.py' or 'src/**/*.js' """,
                required=True,
            ),
        ],
    ),
    "loc_count": MeasurementDef(
        obj=LocCountMeasurement,
        params=[
            ParamOption(
                name="languages",
                description="""The languages to pass to tokei. eg: 'jsx,tsx,js,ts' """,
                required=False,
            ),
            ParamOption(
                name="total",
                description="""Whether to sum the total lines of code.""",
                required=False,
            ),
        ],
    ),
    "custom_script": MeasurementDef(
        obj=ScriptMeasurement,
        params=[
            ParamOption(
                name="command",
                description="""A shell command to run, like 'wc -l *', or the path to a script you've written: './my_script.sh'""",
                required=True,
            ),
            ParamOption(
                name="return_type",
                description="""The return type. Either 'number' (the command prints a single number), or 'json' (the command prints a line of JSON).""",
                required=False,
            ),
        ],
    ),
}
