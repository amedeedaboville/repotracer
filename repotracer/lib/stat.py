import pandas as pd
from . import git
from tqdm.auto import tqdm
from datetime import datetime, date

from .config import RepoConfig, StatConfig
from .measurement import Measurement, all_measurements
from .storage import Storage, CsvStorage
from .plotter import plot
from typing import Callable
from dataclasses import dataclass
import os
import logging
from rich import print

logger = logging.getLogger(__name__)
logger.setLevel(logging.INFO)
logger.addHandler(logging.StreamHandler())
# logger.addHandler(logging.StreamHandler())o
# logging.config.dictConfig(
#     {
#         "disable_existing_loggers": True,
#     }
# )


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
        self.measurement = all_measurements[stat_params["type"]].obj(
            stat_params["params"]
        )
        self.stat_name = stat_params["name"]
        self.description = stat_params["description"]
        self.path_in_repo = stat_params.get("path_in_repo")
        self.start = stat_params.get("start")

    def cd_to_repo_and_setup(self, repo_path):
        logger.debug(f"cd from {os.getcwd()} to {repo_path}")
        os.chdir(repo_path)
        # todo this is slow on large repos
        # maybe only do it if there are untracked files, or do it manually
        # git.clean_untracked()
        git.reset_hard("HEAD")
        branch = self.repo_config.default_branch or "master"
        git.checkout(branch)
        git.pull(branch)
        if self.path_in_repo:
            os.chdir(self.path_in_repo)

    def loop_through_commits_and_measure(self, commits_to_measure):
        commit_stats = []
        # We assume we have already cd'd to the right place to measure the stat
        stat_measuring_path = os.getcwd()
        for commit in (
            pbar := tqdm(
                commits_to_measure.itertuples(index=True), total=len(commits_to_measure)
            )
        ):
            pbar.set_postfix_str(commit.Index.strftime("%Y-%m-%d"))
            if not commit.sha:
                commit_stats.append({"date": commit.Index})
                continue
            git.reset_hard(commit.sha)
            os.chdir(stat_measuring_path)
            stat = {
                "sha": commit.sha,
                "date": commit.Index,
                **self.measurement(),
            }
            commit_stats.append(stat)

        return (
            pd.DataFrame(commit_stats)
            .ffill()
            .set_index("date")
            .tz_localize(None)
            .convert_dtypes()
        )

    def run(self):
        previous_cwd = os.getcwd()
        repo_path = "./repos/" + self.repo_config.path
        repo_name = self.repo_config.name
        if not git.is_repo_setup(repo_path):
            # todo maybe don't try to download it, just error or tell them to run repotracer add repo
            raise Exception(
                f"Repo '{repo_name}' not found at {repo_path}. Run `repotracer add repo {repo_name}` to download it."
            )

        existing_df = CsvStorage().load(self.repo_config.name, self.stat_name)
        end = datetime.today()
        agg_config = self.agg_config or AggConfig(
            time_window="D", agg_fn=None, agg_window=None
        )

        self.cd_to_repo_and_setup(repo_path)
        start = self.find_start_day(existing_df)
        commits_to_measure = git.build_commit_df(start, end, agg_config.time_window)
        if len(commits_to_measure) == 0:
            logger.info(f"No commits found in the time window {start}-{end},  skipping")
            os.chdir(previous_cwd)
            return
        logger.info(f"Going from {start} to {end}, {len(commits_to_measure)} commits")
        new_df = self.loop_through_commits_and_measure(commits_to_measure)

        if agg_config.agg_fn:
            new_df.groupby(
                pd.Grouper(key="created_at", agg_freq=agg_freq), as_index=False
            ).agg(agg_config.agg_fn)

        if existing_df is not None:
            df = new_df.combine_first(existing_df)
        else:
            df = new_df

        os.chdir(previous_cwd)
        CsvStorage().save(self.repo_config.name, self.stat_name, df)
        plot(
            self.repo_config.name,
            self.stat_name,
            self.description,
            df,
            run_at=datetime.now(),
        )

    def find_start_day(self, df) -> date:
        # We need to ask the storage engine for the current version of the data
        # It should give us a df, and we can use that to find the latest days missing
        if df is None or df.empty:
            if self.start:
                start = pd.to_datetime(self.start)
                logger.debug(f"Using given self.start: {self.start}")
            else:
                first_commit = git.first_commit_date()
                logger.debug(f"Found first commit date {first_commit}")
                start = first_commit
            logger.info(
                f"No existing data found, starting from the beginning on {start}"
            )
        else:
            start = df.index.max() - pd.Timedelta(days=1)
            logger.debug(f"Found existing data date {start}")
        # Return a list of days missing
        return start
