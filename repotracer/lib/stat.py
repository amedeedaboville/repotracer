import pandas as pd
from . import git
from tqdm.auto import tqdm
from datetime import datetime, date

from .config import RepoConfig, StatConfig
from .measurement import Measurement, all_measurements
from .storage import Storage, CsvStorage
from typing import Callable
from dataclasses import dataclass
import os


@dataclass
class AggConfig(object):
    time_window: str = "D"
    agg_fn: Callable | None = None
    agg_window: str | None = None


def agg_percent(self):
    return self.sum() / len(self)


@dataclass()
class Stat(object):
    repo_config: RepoConfig
    stat_name: str
    start: str | None = None
    end: str | None = None
    measurement: Measurement | None = None
    agg_config: AggConfig | None = None
    path_in_repo: str | None = None

    def __init__(self, repo_config: RepoConfig, stat_params: StatConfig):
        self.repo_config = repo_config
        self.measurement = all_measurements[stat_params["type"]](stat_params["params"])
        self.stat_name = stat_params["name"]
        self.path_in_repo = stat_params.get("path_in_repo")
        self.start = stat_params.get("start")

    def run(self):
        previous_cwd = os.getcwd()
        repo_path = "./repos/" + self.repo_config["path"]
        repo_name = self.repo_config["name"]
        if not git.is_repo_setup(repo_path):
            # todo maybe don't try to download it, just error or tell them to run repotracer add repo
            print(
                f"Repo {repo_name} not found. Downloading it from '{self.repo_config['url']}':"
            )
            git.download_repo(url=self.repo_config["url"], repo_path=repo_path)

        commit_stats = []
        existing_df = CsvStorage().load(self.repo_config["name"], self.stat_name)
        start = self.find_start_day(existing_df)
        end = datetime.today()
        agg_config = self.agg_config or AggConfig(
            time_window="D", agg_fn=None, agg_window=None
        )

        print(os.getcwd())
        print(repo_path)
        os.chdir(repo_path)
        # todo this is slow on large repos
        # git.clean_untracked()
        git.reset_hard_head()
        git.checkout("master")
        git.pull()
        if self.path_in_repo:
            os.chdir(self.path_in_repo)
        full_stat_start_path = os.getcwd()
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
        # pd.date_range(datetime.today(), current_day, freq="D").tolist()
        commits = commits.groupby(
            pd.Grouper(key="created_at", freq=agg_config.time_window)
        ).last()
        if len(commits) == 0:
            print(f"No commits found in the time window for repo {repo_name}, skipping")
            return
        print(f"Going from {start} to {end}, {len(commits)} commits")
        for commit in (
            pbar := tqdm(commits.itertuples(index=True), total=len(commits))
        ):
            pbar.set_postfix_str(commit.Index.strftime("%Y-%m-%d"))
            if not commit.sha:
                commit_stats.append({"date": commit.Index})
                continue
            if self.path_in_repo:
                os.chdir(full_stat_start_path)
            git.checkout(commit.sha)
            stat = {
                "sha": commit.sha,
                "date": commit.Index,
                **self.measurement(),
            }
            commit_stats.append(stat)

        new_df = pd.DataFrame(commit_stats).ffill().set_index("date").tz_localize(None)
        # single_stat = len(df.columns) == 3
        # stat_column = df.columns[2] if single_stat else None
        if agg_config.agg_fn:
            new_df.groupby(
                pd.Grouper(key="created_at", agg_freq=agg_freq), as_index=False
            ).agg(agg_config.agg_fn)

        if existing_df is not None:
            df = new_df.combine_first(existing_df)
        else:
            df = new_df

        os.chdir(previous_cwd)
        CsvStorage().save(self.repo_config["name"], self.stat_name, df)

    def find_start_day(self, df) -> date:
        # We need to ask the storage engine for the current version of the data
        # It should give us a df, and we can use that to find the latest days missing
        if df is None or df.empty:
            start = self.start or git.first_commit_date()
        else:
            start = df.index.max() - pd.Timedelta(days=1)
            print(f"Found existing data date {start}")
        # Return a list of days missing
        return start
