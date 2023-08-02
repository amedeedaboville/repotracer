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


def download_repo(url):
    repo_name = os.path.splitext(os.path.basename(url))[0]
    repo_dir = os.path.join(src_dir, repo_name)
    os.makedirs(repo_dir, exist_ok=True)
    os.chdir(repo_dir)
    if not os.path.exists(os.path.join(repo_dir, ".git")):
        git.init()
    remote_res = git.remote("get-url", "origin")
    print(remote_res)
    if remote_res.exit_code != 0:
        git.remote("add", "origin", url)
    # todo get the default branch first
    git.fetch("origin/master", '--shallow-since="1 year ago"')
    return repo_dir
