from . import git

import subprocess


def script(cmd):
    return lambda: subprocess.check_output(cmd, shell=True)


def script_now(cmd):
    try:
        return subprocess.check_output(cmd, shell=True).decode("utf-8")
    except subprocess.CalledProcessError as e:
        # ignoGenericre exit code 1
        if e.returncode == 1:
            return e.output
        print("Error in script_now")
        print(e)
        print(e.output)
        raise e


def tokei_specific(languages):
    return script(f"tokei --output json --output-file - --languages {languages}")


def ripgrep_count_file(pattern):
    return int(script(f"rg -l {pattern} | wc -l"))


def rg_count(pattern: str, rg_args: str) -> int:
    filenames_with_counts = script_now(f"rg '{pattern}' --count {rg_args}")
    res = {
        "total": sum(
            int(line.split(":")[-1]) for line in filenames_with_counts.splitlines()
        )
    }
    return res


def loc():
    return script_now("tokei --total")


def jsx_to_tsx():
    return script_now("tokei --output json --output-file - --languages jsx,tsx,js,ts")
