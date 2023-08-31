from dataclasses import dataclass
from typing import Callable, Generic, TypeVar, Type, Any, TypedDict
from abc import ABC, abstractmethod
import pandas as pd
from tqdm.auto import tqdm
from datetime import datetime
import functools
from typing_extensions import Protocol
from .measurement_fns import rg_count, fd_count
from collections import namedtuple


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


RegexMeasurement = FunctionMeasurement[RegexConfig](
    lambda config: rg_count(config["pattern"], config.get("ripgrep_args"))
)

FileCountMeasurement = FunctionMeasurement[FileCountConfig](
    lambda config: fd_count(config["pattern"], config.get("fd_args"))
)

# jsx_to_tsx = FunctionMeasurement(tokei_specific(["jsx", "tsx", "js", "ts"]))
# authors_per_month = FunctionMeasurement(git.get_commit_author)

ParamOption = namedtuple("ParamOption", ["name", "description", "required"])
MeasurementDef = namedtuple("MeasurementDef", ["obj", "params"])

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
    "json_returning_script": MeasurementDef(
        obj=None,
        params=[
            ParamOption(
                name="path",
                description="""The path to a script to run. The script print json to stdout.""",
                required=True,
            ),
        ],
    ),
}
