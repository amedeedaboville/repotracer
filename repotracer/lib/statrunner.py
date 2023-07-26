import pandas as pd
from . import git
from tqdm.auto import tqdm
from datetime import datetime

from .stats import Measurement
from typing import Callable
from dataclasses import dataclass


@dataclass
class AggConfig(object):
    time_window: str = "D"
    agg_fn: Callable | None = None
    agg_window: str | None = None


def agg_percent(self):
    return self.sum() / len(self)


@dataclass()
class Stat(object):
    stat_name: str
    start: str | None = None
    end: str | None = None
    measurement: Measurement | None = None
    agg_config: AggConfig | None = None

    def run(self):
        commit_stats = []
        start = self.start or "2022-01-01"  # or git.first_commit_date()
        end = self.end or datetime.today().strftime("%Y-%m-%d")
        agg_config = self.agg_config or AggConfig(
            time_window="D", agg_fn=None, agg_window=None
        )
        git.reset_hard_head()
        git.checkout("master")
        commits = pd.DataFrame(
            git.list_commits(start, end), columns=["sha", "created_at"]
        )
        commits.created_at = pd.DatetimeIndex(
            data=pd.to_datetime(commits.created_at, utc=True)
        )
        commits = commits.set_index(
            commits.created_at,
            drop=False,
        )
        # todo bring this back in when doing different aggregations:
        # if self.computation.time_window:
        commits = commits.groupby(
            pd.Grouper(key="created_at", freq=agg_config.time_window)
        ).last()
        print(f"Going from {start} to {end}, {len(commits)} commits")
        for commit in (
            pbar := tqdm(commits.itertuples(index=True), total=len(commits))
        ):
            if not commit.sha:
                commit_stats.append({"date": commit.Index})
                continue
            pbar.set_postfix_str(commit.Index.strftime("%Y-%m-%d"))
            git.checkout(commit.sha)
            stat = {
                "sha": commit.sha,
                "date": commit.Index,
                **self.measurement(),
            }
            commit_stats.append(stat)
        df = pd.DataFrame(commit_stats).ffill().set_index("date")
        # single_stat = len(df.columns) == 3
        # stat_column = df.columns[2] if single_stat else None
        if agg_config.agg_fn:
            df.groupby(
                pd.Grouper(key="created_at", agg_freq=agg_freq), as_index=False
            ).agg(agg_config.agg_fn)
        return df

    def find_missing_days(self):
        # We need to ask the storage engine for the current version of the data
        # It should give us a df, and we can use that to find the latest days missing
        df = CsvStorage(self.stat_name).load()
        last_date = df.index.max()
        # Return a list of days missing
        return pd.date_range(datetime.today(), current_day, freq="D").tolist()
