from dataclasses import dataclass
from typing import Callable
import subprocess
import pandas as pd
from . import git
from tqdm.auto import tqdm
from datetime import datetime
import functools


# todo find a way to do polymorphism nicely
# by having the statconfig be typed differently
# based on the Stat itself
# @dataclass
# class StatConfig(object):
#     storage: str = "csv"
#     stat_specific_config: object = None


# This is the computation itself
@dataclass
class Computation(object):
    stat_fn: Callable
    time_window: str = "D"
    agg_fn: Callable | None = None
    agg_window: str | None = None


# A user specifies a computation, some parameters to the computation, and some extra parameters (eg the storage)
# Names for these things tbd


def script(cmd):
    return lambda: subprocess.check_output(cmd, shell=True)


def script_now(cmd):
    try:
        return subprocess.check_output(cmd, shell=True).decode("utf-8")
    except subprocess.CalledProcessError as e:
        # ignore exit code 1
        if e.returncode == 1:
            return e.output
        print("Error in script_now")
        print(e)
        print(e.output)
        raise e


def tokei_specific(languages):
    return script(f"tokei --output json --output-file - --languages {languages}")


def ripgrep_count_file(pattern):
    return int(script(f"rg -l {pattern} | wc -l"))


def rg_count(pattern) -> int:
    filenames_with_counts = script_now(f"rg {pattern} --count")
    res = {
        "total": sum(
            int(line.split(":")[-1]) for line in filenames_with_counts.splitlines()
        )
    }
    return res


def agg_percent(self):
    return self.sum() / len(self)


revert_commit_percentage = Computation(
    stat_fn=lambda: {stat: is_revert()},
    agg_window="M",
    agg_fn=lambda df: df["stat"].avg(),
)

daily_loc = Computation(stat_fn=script("tokei --total"))
jsx_to_tsx = Computation(tokei_specific(["jsx", "tsx", "js", "ts"]))
authors_per_month = Computation(git.get_commit_author, time_window="M", agg_fn="count")

# run a ripgrep search for a specific string
def regex_stat(pattern):
    return Computation(stat_fn=(lambda: rg_count(pattern)), time_window="D")


class RegexStat(Computation):
    def __init__(self, pattern: str):
        self.pattern = pattern
        super().__init__(stat_fn=self.stat, time_window="D")

    def stat(self):
        return rg_count(self.pattern)
