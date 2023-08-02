from datetime import datetime
import sh
import os
import logging
import subprocess

# logging.basicConfig(level=logging.INFO)


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
    return git.checkout(sha)


def reset_hard_head():
    return git.reset("--hard", "HEAD")


def clean_untracked():
    return git.clean("-fxd")


def current_message():
    return git.log("-1", "--pretty=%B")


def current_files():
    return git.diff("--name-only", "HEAD~")


def current_date():
    return git.log("-1", "--pretty=format:%cd")


def pull(obj="master"):
    return git.pull("origin", obj)


def get_commit_author():
    return git.log("-1", "--pretty=format:%aE")


def is_repo_setup(repo_path):
    return os.path.exists(os.path.join(repo_path, ".git"))


def download_repo(url, repo_path):
    repo_name = os.path.splitext(os.path.basename(url))[0]
    os.makedirs(repo_path, exist_ok=True)
    os.chdir(repo_path)
    if not os.path.exists(os.path.join(repo_path, ".git")):
        subprocess.run(["git", "init"])
    ret_code = subprocess.run(["git", "remote", "get-url", "origin"]).returncode
    if ret_code != 0:
        subprocess.run(["git", "remote", "add", "origin", url])
    return repo_path
    ret_code = subprocess.run(
        ["git", "fetch", "origin/master", '--shallow-since="1 year ago"']
    )
