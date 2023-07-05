from datetime import datetime
import sh
import os

repodir = "/users/amedee/workspace/samplerepos/react"
git = sh.git.bake(_cwd=repodir, no_pager=True)


def list_commits(start, end):  # -> [string, string]:
    start = start or "2000-01-01"
    end = end or datetime.now().strftime("%Y-%m-%d")
    print(os.getcwd())
    output = git.log(format="%H,%cd", date="iso-strict")  # , after=start, before=end)
    print(output)
    # outsplitlines = output.splitlines()
    # print(outsplitlines[0].split(","))
    return [tuple(line.split(",")) for line in output.splitlines()]


def first_commit_date():
    return git.log("-1", "--pretty=format:%cd", "--date=iso-strict")


def checkout(sha):
    return git.checkout(sha)


def current_message():
    return git.log("-1", "--pretty=%B")


def current_files():
    return git.diff("--name-only", "HEAD~")


def current_date():
    return git.log("-1", "--pretty=format:%cd")


def get_commit_author():
    return git.log("-1", "--pretty=format:%aE")


def download_repo(url):
    repo_name = os.path.splitext(os.path.basename(url))[0]
    repo_dir = os.path.join(src_dir, repo_name)
    os.makedirs(repo_dir, exist_ok=True)
    os.chdir(repo_dir)
    if not os.path.exists(os.path.join(repo_dir, ".git")):
        subprocess.run(["git", "init"])
    ret_code = subprocess.run(["git", "remote", "get-url", "origin"]).returncode
    if ret_code != 0:
        subprocess.run(["git", "remote", "add", "origin", url])
    return repo_dir
    ret_code = subprocess.run(
        ["git", "fetch", "origin/master", '--shallow-since="1 year ago"']
    )
