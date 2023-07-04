import git
from datetime import date, timedelta, datetime
from tqdm import tqdm
import json
import os
import pandas as pd
import subprocess
import sh
import sys
from stats import *


def last_year_days():
    return [date.today() - timedelta(days=d) for d in range(365)]


def tokei_runner():
    (code, out) = subprocess.getstatusoutput(
        "tokei -o json | jq 'with_entries(.value = .value.code)'"
    )
    return json.loads(out)


# example_stat = (
#     stat_builder()
#     .time_window("D")
#     .agg_freq("M")
#     .agg_fn(lambda x: x)
#     .start()
#     .end("tomorrow")
# )


# class stat:
#     def start(self, start):
#         self.start = start

#     def __call__(self):
#         commit_stats = []
#         commits = pd.DataFrame(list_commits(start, end)).set_index("created_at")
#         if time_window:
#             commits = commits.groupBy(
#                 pd.Grouper(key=created_at, freq=time_window)
#             ).last()
#         for commit in tqdm(commits.itertuples(index=false)):
#             git.checkout(commit.sha)
#             stat = {
#                 "sha": commit,
#                 "date": commit.created_at,  # or git.current_date()
#                 **stat_fn(commit),
#             }
#             commit_stats.append(stat)
#         df = pd.DataFrame(commit_stats).set_index("date")
#         # single_stat = len(df.columns) == 3
#         # stat_column = df.columns[2] if single_stat else None
#         if agg_fn:
#             df.groupBy(
#                 pd.Grouper(key="created_at", agg_freq=agg_freq), as_index=False
#             ).agg(agg_fn)
#         return df


from stats import daily_loc

example_stat = daily_stat(stat_fn=daily_loc, start="1 year ago")
example_stat()
