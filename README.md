## Repotracer: Watch your code changing over time

Repotracer gives you insight into the change going on in your codebase.

It will loop through every day in the history of a git repository, and collect any stat you might ask it to. Eg:

- Typescript migration: count LOC of JS and TS
- Count uses of deprecated functions
- Measure adoption of new APIs

It compiles the results for each day into a csv, and also immediately gives you a plot of the data in csv.

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
