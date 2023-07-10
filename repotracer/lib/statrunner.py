import pandas as pd
from . import git
from tqdm.auto import tqdm
from datetime import datetime

from .stats import Stat
from dataclasses import dataclass


@dataclass
class StatRunner(object):
    stat: Stat
    start: str | None = None
    end: str | None = None

    def run(self):
        start = self.start
        end = self.end
        time_window = self.stat.time_window
        commit_stats = []
        if start is None:
            start = "2022-01-01"
            # start = git.first_commit_date()
        if end is None:
            end = datetime.today().strftime("%Y-%m-%d")
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
        # if time_window:
        commits = commits.groupby(pd.Grouper(key="created_at", freq=time_window)).last()
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
                **self.stat.stat_fn(),
            }
            commit_stats.append(stat)
        df = pd.DataFrame(commit_stats).ffill().set_index("date")
        # single_stat = len(df.columns) == 3
        # stat_column = df.columns[2] if single_stat else None
        if self.stat.agg_fn:
            df.groupby(
                pd.Grouper(key="created_at", agg_freq=agg_freq), as_index=False
            ).agg(self.stat.agg_fn)
        return df

    def find_missing_days():
        pass
