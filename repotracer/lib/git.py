from datetime import datetime
import sh
import os
import sys
import logging
import subprocess
from .config import RepoConfig
from . import config
from rich.console import Console
from typing import Optional
import pandas as pd

# logging.basicConfig(level=logging.INFO)


def check():
    revparse = git("rev-parse", "--show-toplevel").strip()
    if revparse.endswith("repotracer"):
        print("raising")
        raise Exception(
            "(bug in repotracer): cannot run a git command against the repotracer repo itself"
        )


repodir = "/users/amedee/workspace/samplerepos/react"
git = sh.git.bake(no_pager=True)
git_normal = sh.git.bake(_out=sys.stdout, _err=sys.stderr)


def list_commits(start, end):  # -> [string, string]:
    start = start or first_commit_date()
    end = end or datetime.now().strftime("%Y-%m-%d")
    data = []
    for line in git.log(
        format="%H,%cd",
        date="iso-strict",
        since=start,
        until=end,
        no_merges=True,
        _iter=True,
    ):
        data.append(line.split(","))
    return data


def build_commit_df(
    start: Optional[str], end: Optional[str], freq: str = "D"
) -> pd.DataFrame:
    commits = pd.DataFrame(list_commits(start, end), columns=["sha", "created_at"])
    commits.created_at = pd.DatetimeIndex(
        data=pd.to_datetime(commits.created_at, utc=True)
    )
    commits = commits.set_index(
        commits.created_at,
        drop=False,
    )
    commits = commits.groupby(
        pd.Grouper(key="created_at", freq=freq)
    ).last()
    return commits


def first_commit_date():
    check()
    # from https://stackoverflow.com/a/5189296
    first_sha = git("rev-list", "--max-parents=0", "HEAD").strip()
    return git.log(first_sha, "--pretty=format:%cd", "--date=format:%Y-%m-%d")


def checkout(sha):
    check()
    return git.checkout(sha)


def reset_hard(target="HEAD"):
    check()
    return git.reset("--hard", target)


def clean_untracked():
    check()
    return git.clean("-fxd")


def current_message():
    check()
    return git.log("-1", "--pretty=%B")


def current_files():
    check()
    return git.diff("--name-only", "HEAD~")


def current_date():
    check()
    return git.log("-1", "--pretty=format:%cd")


def pull(obj="master"):
    check()
    return git.pull("origin", obj)


def get_commit_author():
    check()
    return git.log("-1", "--pretty=format:%aE")


def is_repo_setup(repo_path):
    return os.path.exists(os.path.join(repo_path, ".git"))


def get_default_branch():
    res = git("symbolic-ref", "refs/remotes/origin/HEAD")
    # return the basename of res
    return os.path.basename(res.strip())


def download_repo(url, name=None, branch=None):
    cwd = os.getcwd()
    repos_location = config.get_repo_storage_location()
    repo_name = name or os.path.splitext(os.path.basename(url))[0]
    repo_storage_path = os.path.join(repos_location, repo_name)
    os.makedirs(repo_storage_path, exist_ok=True)
    print(f"Downloading {repo_name} from {url} to {repo_storage_path}")
    os.chdir(repos_location)

    # This is a  "blobless" clone, which downloads the data for the latest commit (HEAD) and then for past commits
    # downloads only the metadata (commit summaries and then trees which are folder names + pointers to blobs which contain file contents)
    # It's useful to us because we are going to be checking out only a fraction of the commits, so
    # we won't need the file contents for the rest of the commits.
    # git_normal.clone(url, ".", "--filter=blob:none", _out=sys.stdout, _err=sys.stderr)
    # todo decide if we actually want a blobless ^

    if not branch:
        git_normal.clone(url, repo_name, "--single-branch")
    else:
        git_normal.clone(
            url,
            repo_name,
            "--single-branch",
            f"--branch={branch}",
        )

    os.chdir(repo_name)
    # after we do the blobless clone we check out the last commit of every day, to make git
    # download the blobs for those commits, so that way the first stat run won't have to do it.
    # This is technically part of downloading the repo
    # for commit in list_commits():
    #     sha = commit[0]
    #     git.checkout(sha)

    default_branch = branch or get_default_branch()
    os.chdir(cwd)
    return RepoConfig(name=repo_name, path=repo_name, default_branch=default_branch)
