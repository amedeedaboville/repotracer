from dataclasses import dataclass
from typing import Callable, Generic, TypeVar, Type, Any, TypedDict
from abc import ABC, abstractmethod
import pandas as pd
from tqdm.auto import tqdm
from datetime import datetime
import functools
from typing_extensions import Protocol
from .measurements import rg_count


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


def function_measurement(fn: Callable[[TConfig], list[Any]]):
    return FunctionMeasurement(fn)


# def functional_measurement(fn: Callable[[TConfig], list[Any], config: TConfig):
#     def fn_with_config():
#         return fn(config)
#     return fn_with_config()


def regex(pattern: str):
    return rg_count(pattern)


RegexMeasurement = FunctionMeasurement[RegexConfig](
    lambda config: rg_count(config["pattern"])
)

# jsx_to_tsx = FunctionMeasurement(tokei_specific(["jsx", "tsx", "js", "ts"]))
# authors_per_month = FunctionMeasurement(git.get_commit_author)


# @stat("regex_count")
# class RegexMeasurement(Measurement):
#     config: RegexConfig

#     def run(self) -> list[Any]:
#         return rg_count(self.config.pattern)
all_measurements = {
    "regex_count": RegexMeasurement,
}
