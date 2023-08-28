from datetime import datetime
import sh
import os
import logging
import subprocess
from .config import RepoConfig

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


def download_repo(url, name=None):
    repo_name = name or os.path.splitext(os.path.basename(url))[0]
    repo_storage_path = os.path.join("./repos", repo_name)
    print(f"Downloading {repo_name} from {url} to {repo_storage_path}")
    os.makedirs(repo_storage_path, exist_ok=True)
    cwd = os.getcwd()
    os.chdir(repo_storage_path)
    git.clone(url, ".", "--shallow-since=1 year ago")
    # if not os.path.exists(os.path.join(repo_path, ".git")):
    #     git.init()
    # try:
    #     git.remote("get-url", "origin")
    # except sh.ErrorReturnCode_2 as e:
    #     git.remote("add", "origin", url)

    # try:
    #     git.pull("origin", '--shallow-since="1 year ago"')
    # except sh.ErrorReturnCode_1 as e:
    #     print("Shallow fetch failed, trying a full fetch.")
    #     git.pull("origin")

    default_branch = get_default_branch()
    os.chdir(cwd)
    return RepoConfig(name=repo_name, path=repo_name, default_branch=default_branch)
