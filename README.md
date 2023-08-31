## Repotracer: Watch your code changing over time

Repotracer gives you insight into the change going on in your codebase.

It will loop through every day in the history of a git repository, and collect any stat you might ask it to. Eg:

- Typescript migration: count the number of lines of JS vs TS
- Count number of deprecated function calls
- Measure adoption of new APIs

It compiles the results for each day into a csv, and also immediately gives you a plot of the data in csv.
It supports incremental computation: re-run it every so often without starting from scratch.

Use it to watch:

- Percent of commits that touched at least one test, and count of authors writing tests
- Count number of authors who have used a new library

These are only the beginning. You can write your own stats and plug them into repotracer. If you can write a script to calculate a property of your code, then repotracer can graph it for you over time. For example you could run your build toolchain and counting numbers of a particular warning, or use a special tool.

Repotracer aims to be a swiss army knife to run analytics queries on your source code.

### Installation

Install with `pip install repotracer`.

To run the `regex_count` and `file_count` stats, you'll need to have `ripgrep` and `fd` installed, respectively. On Macos you can install these with:

```
brew install ripgrep fd
```

Repotracer will look for a file named `config.json` in the current directory, so pick a directory you want to run it from, and always run it from there. This might change in the future, but for now it allows for different configs.

```
mkdir traces && cd traces
repotracer add-stat
```

`add-stat` will guide you through the process of configuring a repo, and adding a new stat.

### Usage

A collection of commands for onboarding will come soon. In the meantime:

- `repotracer run reponame statname` will compute a single stat. The data will show up in `./stats/repo/stat_name.csv`, and a plot will be written to `./stats/repo/stat_name.png`.

- `repotracer run reponame` will run all the stats for that repo. For now this makes separate passes for each commit, later it might do several stats for the same commit at a time.

- `repotracer run` will update all stats in the config file.

## Stat types

More documentation about the configuration options will come soon.

- `regex_count` runs ripgrep and sums the number of matches in the whole repo. Additional args can be passed to ripgrep by adding `rg_args` in the `params` object.
- `file_count` runs `fd` and counts the number of files found.
- The next stat will be `script`, which will run any bash script the user will want, to allow for max customization.

## Stat options

The config format is JSON5, but currently comments are lost when the command updates the config file.

```
"repos" : {
    "svelte": {
      "url": "", // the url to clone the repo from
      "stats": {
        "count-ts-ignore": { // the name of the stat. Will be used in filenames
          "description": "The number of ts-ignores in the repo.", //Optional. A short description of the stat.
          "type": "regex_count", //
          "start": "2020-01-01", // Optional. When to start the the collection for this stat. If not specified, will use the beginning of the repo
          "path_in_repo": "2020-01-01", // Optional. Will cd into this path to run the stat
          "params": { // any parameters that depend on the measurement type
            "pattern": "ts-ignore",  //The pattern to pass to rigpgrep
            "ripgrep_args": "-g '!**/tests/*'" // any extra arguments to pass to ripgrep
          }
        },
      }
    }
}
``
```
