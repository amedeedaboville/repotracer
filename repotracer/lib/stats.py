import subprocess
import pandas as pd
from .git import git
from tqdm import tqdm


def stat(stat_fn, time_window=None, start=None, end=None, agg_fn=None, agg_freq=None):
    def compute():
        commit_stats = []
        commits = pd.DataFrame(
            git.list_commits(start, end), columns=["sha", "created_at"]
        )
        print(len(commits))
        # commits.created_at = commits.created_at.astype(pd.DatetimeIndex())
        commits.created_at = pd.DatetimeIndex(
            data=pd.to_datetime(commits.created_at, utc=True)
        )
        # print(commits.created_at)
        # print(commits.dtypes)
        commits = commits.set_index(
            commits.created_at,
            drop=False,
        )
        print(commits[1:10])
        print(commits.size)
        print(commits.dtypes)
        if time_window:
            commits = commits.groupby(
                pd.Grouper(key="created_at", freq=time_window)
            ).last()
        for commit in tqdm(commits.itertuples(index=False)):
            if not commit:
                break
            print(commit)
            git.checkout(commit.sha)
            stat = {
                "sha": commit,
                "date": commit.created_at,  # or git.current_date()
                **stat_fn(commit),
            }
            commit_stats.append(stat)
        df = pd.DataFrame(commit_stats).set_index("date")
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
    return lambda: subprocess.check_output(cmd, shell=True, cwd=repo_dir)


def script_now(cmd):
    return lambda: subprocess.check_output(cmd, shell=True, cwd=repo_dir)


def tokei_specific(languages):
    return script(f"tokei --output json --output-file - --languages {languages}")


def ripgrep_count_file(pattern):
    return int(script(f"rg -l {pattern} | wc -l"))


def rg_count(pattern) -> int:
    filenames_with_counts = script_now(f"rg {pattern} --count")
    return sum(line.split(":")[-1] for line in filenames_with_counts.splitlines())


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

regex_over_time = stat(lambda: rg_count("time_t"), time_window="D")
