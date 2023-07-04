import re


def is_revert():
    return len(re.search("REVERT .", get_commit_message())) > 0


def touched_file(files):
    return git.current_files().contains(files)
