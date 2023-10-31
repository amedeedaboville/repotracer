from datetime import datetime
import sh
import shutil
import os
import sys
import logging
import re
import subprocess
from .config import RepoConfig
from . import config
from rich.console import Console
from typing import Optional
from pathlib import PurePath
import pandas as pd

# logging.basicConfig(level=logging.INFO)


# This check was added when the stat runner was cding around too much
# I removed it so that we could dogfood repotracer by running it on itself
def check():
    pass
    revparse = git("rev-parse", "--show-toplevel").strip()
    in_repos_dir = PurePath(os.getcwd()).is_relative_to(config.get_repos_dir())
    if revparse.endswith("repotracer") and not in_repos_dir:
        raise Exception(
            "(bug in repotracer): cannot run a git command against the repotracer repo itself"
        )


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
    commits = commits.groupby(pd.Grouper(key="created_at", freq=freq)).last()
    return commits


def first_commit_date():
    check()
    # from https://stackoverflow.com/a/5189296
    # Note that that there might be more than one "root" commit
    git_results = git("rev-list", "--max-parents=0", "HEAD").strip()
    # For now we assume that the last one in the list is the earliest, it worked
    # for torvalds/linux
    first_sha = git_results.split("\n")[-1]

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
    # There is no 'canonical' method of getting the default branch
    # cf https://stackoverflow.com/questions/28666357/how-to-get-default-git-branch
    # This is similar to the sed answer ^
    git_output = git("remote", "show", "origin")
    print(git_output)
    try:
        res = re.search("HEAD branch: (.*)", git_output).group(1)
    except Error:
        print(
            "Could not find default branch, guessing 'master'. If this is incorrect, set the default_branch key on the repo in config.json"
        )
        res = "master"
    return res


def is_url(x):
    try:
        result = urlparse(x)
        return all([result.scheme, result.netloc])
    except:
        return False

    print("default branch is ", default_branch, " checking it out")
    """
            # todo make a more full featured version of this in the git module
        source_path = os.path.expanduser(url_or_path)
        repo_name = name or os.path.basename(source_path)
        repo_storage_path = os.path.join("./repos", repo_name)
        repo_git_dir = os.path.join(repo_storage_path, ".git")
        cwd = os.getcwd()
        os.makedirs(repo_git_dir, exist_ok=True)
        git_dir = os.path.join(source_path, ".git")
        with console.status(f"Copying {git_dir} into '{repo_storage_path}'..."):
            shutil.copytree(git_dir, repo_git_dir, dirs_exist_ok=True)
        os.chdir(repo_storage_path)
        git.checkout(".")
        default_branch = git.get_default_branch()
        git.checkout(default_branch)
        os.chdir(cwd)
        repo_config = config.RepoConfig(
            name=repo_name, path=repo_name, default_branch=default_branch
        )


"""


def download_repo(url_or_path, branch=None):
    cwd = os.getcwd()

    if is_url(url_or_path):
        # splitext to remove the .git off the end
        repo_name = os.path.splitext(os.path.basename(url_or_path))[0]
        repo_source_path = url_or_path
    else:
        # possibly add expanduser here?
        repo_source_path = os.path.abspath(url_or_path)
        repo_name = os.path.basename(repo_source_path)

    repo_storage_path = os.path.join(config.get_repos_dir(), repo_name)
    if os.path.exists(repo_storage_path):
        print(f"Repo {repo_name} already downloaded into {repo_storage_path}")
        return RepoConfig(
            name=repo_name,
            source=repo_source_path,
            storage_path=None,
            default_branch=None,
        )
    os.makedirs(repo_storage_path, exist_ok=True)
    print(f"Cloning {repo_name} from {repo_source_path} to {repo_storage_path}")
    os.chdir(config.get_repos_dir())

    if is_url(url_or_path):
        git_normal.clone(repo_source_path, repo_name, "--single-branch")
    else:
        source_git_dir = os.path.join(repo_source_path, ".git")
        target_git_dir = os.path.join(repo_storage_path, ".git")
        os.makedirs(target_git_dir, exist_ok=True)
        # with console.status(f"Copying {source_git_dir} into '{target_git_dir}'..."):
        print(f"Copying {source_git_dir} into '{target_git_dir}'...")
        shutil.copytree(source_git_dir, target_git_dir, dirs_exist_ok=True)
        os.chdir(repo_storage_path)
        git.checkout(".")
        print(git.status())

    default_branch = get_default_branch()
    git.checkout(default_branch)

    os.chdir(cwd)
    return RepoConfig(
        name=repo_name,
        source=repo_source_path,
        storage_path=None,
        default_branch=default_branch,
    )
