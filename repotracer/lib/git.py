from datetime import datetime
import sh
import os
import logging
import subprocess

# logging.basicConfig(level=logging.INFO)


def check():
    if git("rev-parse", "--show-toplevel").endswith("repotracer"):
        raise Exception(
            "(bug in repotracer): cannot run a git command against the repotracer repo itself"
        )


repodir = "/users/amedee/workspace/samplerepos/react"
git = sh.git.bake(no_pager=True)


def list_commits(start, end):  # -> [string, string]:
    start = start or "2000-01-01"
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


def first_commit_date():
    return git.log("-1", "--pretty=format:%cd", "--date=format:%Y-%m-%d")


def checkout(sha):
    check()
    return git.checkout(sha)


def reset_hard_head():
    check()
    return git.reset("--hard", "HEAD")


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


def download_repo(url, repo_path):
    repo_name = os.path.splitext(os.path.basename(url))[0]
    os.makedirs(repo_path, exist_ok=True)
    cwd = os.getcwd()
    os.chdir(repo_path)
    if not os.path.exists(os.path.join(repo_path, ".git")):
        git.init()
    try:
        git.remote("get-url", "origin")
    except sh.ErrorReturnCode_2 as e:
        git.remote("add", "origin", url)
    # todo get the default branch first
    try:
        git.pull("origin", "master", '--shallow-since="1 year ago"')
    except sh.ErrorReturnCode_1 as e:
        print("Shallow fetch failed, trying a full fetch.")
        git.pull("origin", "master")
    os.chdir(cwd)
    return repo_path
