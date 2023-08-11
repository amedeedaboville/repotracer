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

For now this is a cumbersome affair. A pip install will come soon.

- Clone this repo
- Install poetry
- Run `poetry install`
- Now you can run `repotracer`

### Usage

A collection of commands for onboarding will come soon. In the meantime:

- Copy the `sampleconfig.json` file to `config.json`, and update the `stats` value in the JSON.

- Add a new repo under `repos`, and a new stat under that object's `stats`.
  ATM the only type of stat is `regex_count`:

- Run `repotracer run` to compute the stat. The data will show up in `./stats/stat_name.csv`, and a plot will be written to `./stats/stat_name.png`.

-

## Stat types

More documentation about the configuration options will come soon.

- `regex_count` runs ripgrep (needs to be installed ahead of time, eg `homebrew install ripgrep`)
- The next stat will be `script`, which will run any bash script the user will want, to allow for max customization.

## Stat options

```
"repos" : {
    "svelte": {
      "url": "", // the url to clone the repo from
      "stats": {
        "count-ts-ignore": { // the name of the stat. Will be used in filenames
          "description": "The number of ts-ignores in the repo.", //Optional. A short description of the stat.
          "type": "regex_count", //
          "start": "2020-01-01", // Optional. When to start the the collection for this stat. If not specified, will use the beginning of the repo
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
