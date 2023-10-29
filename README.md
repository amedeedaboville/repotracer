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

## Dev Notes

This is a side project with no reliability guarantees. It also is optimized for the author's productivity/engagement. It uses heavyweight "dev-friendly" libraries, and doesn't focus too much on code cleanliness.
The main priority is to get value now (the nice data/graphs), rather than build a timeless masterpiece.

That doesn't mean it's meant as a filthy mess. Here are the main concepts:

Repotracer manages a a collection of `Stat` objects. These are specified by a
`StatConfig`, and they mostly define some params + the `Measurement` (ie the actual command to run, like `tokei`, `ripgrep`, a custom script, etc).

A `Stat` can run itself, and that will update the csv for that stat.
From the POV a user, they care about running many `Stat`s on a single repo,
so we aggregate those into a `RepoConfig`. This mainly defines where to download the repo. There is the idea that the repo storage could be pluggable, eg if
the repo is stored on NFS or something different.

The overall config object is `GlobalConfig`, and it composes a couple basic parameters plus a list of `RepoConfig`s.

In theory a bunch of things can be made pluggable, but we'll wait until we need to swap anything out to define the interfaces.

We use pandas to store & interface with the data, for ease of use. Pandas gives day-aggregation functions, and dataframe powers.
