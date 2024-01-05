from . import git
from .config import RepoConfig, StatConfig, get_repos_dir, get_repo_storage_path
from .measurement import Measurement, all_measurements
from .plotter import plot
from .storage import Storage, CsvStorage

from dataclasses import dataclass
from datetime import datetime, date
from tqdm.auto import tqdm
from typing import Callable
import logging
import os
import pandas as pd

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


def cd_to_repo_and_setup(repo_path, path_in_repo, branch="master"):
    logger.debug(f"cd from {os.getcwd()} to {repo_path}")
    os.chdir(repo_path)
    # todo this is slow on large repos
    # maybe only do it if there are untracked files, or do it manually
    # git.clean_untracked()
    git.reset_hard("HEAD")
    git.checkout(branch)
    git.pull(branch)
    if path_in_repo:
        os.chdir(path_in_repo)


def loop_through_commits_and_measure(measure_fn, commits_to_measure):
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
            **measurement_fn(),
        }
        commit_stats.append(stat)

    return (
        pd.DataFrame(commit_stats)
        .ffill()
        .set_index("date")
        .tz_localize(None)
        .convert_dtypes()
    )


def run_stat(repo_config: RepoConfig, stat_params: StatConfig):
    stat_name = stat_params.name
    repo_name = repo_config.name
    repo_path = get_repo_storage_path(repo_name)

    if not git.is_repo_setup(repo_path):
        raise Exception(
            f"Repo '{repo_name}' not found at {repo_path}. Run `repotracer install-repos` to fetch it."
        )

    existing_df = CsvStorage().load(repo_name, stat_name)

    previous_cwd = os.getcwd()
    # We cd into the repo first so we can calculate the start and end dates we need our
    cd_to_repo_and_setup(
        repo_path, stat_params.path_in_repo, branch=repo_config.default_branch
    )

    start = find_start_day(start_params.start, existing_df)
    end = stat_params.end or datetime.today()

    # In the future a StatConfig will be able to specify an acc_config, for eg daily/weekly/monthly
    # or for average results over a day/week/month
    # For now we only support daily, last commit of the day
    agg_config = AggConfig(time_window="D", agg_fn=None, agg_window=None)
    commits_to_measure = git.build_commit_df(start, end, agg_config.time_window)
    if len(commits_to_measure) == 0:
        logger.info(f"No commits found in the time window {start}-{end},  skipping")
        os.chdir(previous_cwd)
        return
    logger.info(f"Going from {start} to {end}, {len(commits_to_measure)} commits")

    measurement_fn = all_measurements[stat_params.type].obj(stat_params.params)
    new_df = loop_through_commits_and_measure(measurement_fn, commits_to_measure)

    if agg_config.agg_fn:
        new_df.groupby(
            pd.Grouper(key="created_at", agg_freq=agg_freq), as_index=False
        ).agg(agg_config.agg_fn)

    if existing_df is not None:
        df = new_df.combine_first(existing_df)
    else:
        df = new_df

    os.chdir(previous_cwd)
    CsvStorage().save(repo_name, stat_name, df)
    plot(
        repo_name,
        stat_name,
        stat_params.description,
        df,
        run_at=datetime.now(),
    )


def find_start_day(start_in_config, df) -> date:
    # We need to ask the storage engine for the current version of the data
    # It should give us a df, and we can use that to find the latest days missing
    if df is None or df.empty:
        if start_in_config:
            start = pd.to_datetime(start_in_config)
            logger.debug(f"Using given start from config: {start_in_config}")
        else:
            first_commit = git.first_commit_date()
            logger.debug(f"Found first commit date {first_commit}")
            start = first_commit
        logger.info(f"No existing data found, starting from the beginning on {start}")
    else:
        start = df.index.max() - pd.Timedelta(days=1)
        logger.debug(f"Found existing data date {start}")
    # Return a list of days missing
    return start
