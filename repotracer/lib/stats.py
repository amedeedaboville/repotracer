import subprocess
import pandas as pd
from . import git
from tqdm.auto import tqdm
from datetime import datetime
import functools


def stat(stat_fn, time_window=None, start=None, end=None, agg_fn=None, agg_freq=None):
    def compute():
        nonlocal stat_fn, time_window, start, end, agg_fn, agg_freq
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
                **stat_fn(),
            }
            commit_stats.append(stat)
        df = pd.DataFrame(commit_stats).ffill().set_index("date")
        # single_stat = len(df.columns) == 3
        # stat_column = df.columns[2] if single_stat else None
        if agg_fn:
            df.groupby(
                pd.Grouper(key="created_at", agg_freq=agg_freq), as_index=False
            ).agg(agg_fn)
        return df

    return compute


def daily_stat(**kwargs):
    return stat(time_window="D", **kwargs)


def script(cmd):
    return lambda: subprocess.check_output(cmd, shell=True)


def script_now(cmd):
    try:
        return subprocess.check_output(cmd, shell=True).decode("utf-8")
        # ignore exit code 1
    except subprocess.CalledProcessError as e:
        if e.returncode == 1:
            # print("Ignoring exit code 1")
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


revert_commit_percentage = lambda: stat(
    stat_fn=lambda: {stat: is_revert()},
    agg_window="M",
    agg_fn=lambda df: df["stat"].avg(),
)

daily_loc = stat(script("tokei --total"), time_window="D")

authors_per_month = stat(git.get_commit_author, time_window="M", agg_fn="count")

jsx_to_tsx = lambda: stat(tokei_specific(["jsx", "tsx", "js", "ts"]), time_window="D")

## stats to write

# run a ripgrep search for a specific string

# regex_over_time = stat(lambda: rg_count("time_t"), time_window="D")
# use functools partial to leave the parameters to stat open
def regex_stat(pattern):
    return functools.partial(stat(lambda: rg_count(pattern), time_window="D"))
