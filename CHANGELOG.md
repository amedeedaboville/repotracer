# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.0] - 2025-08-18

## [Breaking]

- The `measure_file()` interface now takes raw bytes (`&[u8]`) instead of strings (`&str`), better matching what we get from git instead.

### Added

- Add "custom script" file measurement that can run a given shell script over the files that match a pattern.
- Add jq stat collector using the jaq library.
- Add `detect-tools` command that takes a repo path, and looks for particular files tools use like Cargo.toml->Cargo, package.json->npm, etc. Helps to detect which tools a repo might be using to run tool-specific stats.

### Minor

- Upgraded some dependencies, most notably gitoxide which provides a faster zlib-rs, for some modest performance improvements.

## [0.6.3] - 2025-05-17

- Be more flexible in language names, allowing both "Cpp" and "C++". We allow both the "inner"
  language name and the tokei public name.

## [0.6.0] - 2025-04-06

- Fixed the race conditions leading to spikes in the charts by rewriting the aggregation logic, which also gave a 5% overall boost.

## [0.5.0] - 2025-03-21

- Officially made the Rust branch the default version. Things might not port over well or might be broken from the python version.

### Removed

- This removes the plotting support as Rust's plotting support is very weak.

### Fixed

- The new tool is now 10-30x more performant, through parallel processing, use of the gitx library and a new processing approach that reuses results from previous versions of files.
- Tools like tokei, ripgrep and fd don't need to be installed anymore. They are written in Rust so we just use them as libraries.

## [0.4.3] - 2023-11-02

- Fixed bug in previous release.

## [0.4.2] - 2023-11-02

### Fixed

- Fixed bug in `custom_script` where `return_type: "json"` wasn't working.

## [0.4.1] - 2023-11-01

### Fixed

- Fixed bug in `loc_count` stat when passed particular languages.

## [0.4.0] - 2023-11-02

### Added

- New `loc_count` stat type powered by `tokei`. Can take a subset of languages to run for, and can graph/collect a language breakdown or the total
- New `custom_script` stat type. Users can put in a bash command like `wc -c' or a call to a script. The command can return either a single number of a line of json. A future version might allow for CSV output or multiple rows of data.

### Fixed

- Fixed error being thrown when user attempted to run a missing stat, instead of displaying the available stats to run.

## [0.3.1] - 2023-10-28

### Added

- New `install-repos` command to download any repos in the config that have not been cloned yet. (Might be removed later on in favor of auto-downloading repos during `run` or a `refresh-repos` command that either installs or clones)

### Fixed

- Repotracer will first look for a local `$pwd/.repotracer` folder for its config file, then fallback to `$HOME/.repotracer` otherwise. This is a feature so that invidual repos/OSS projects can bundle repotracer configs alongside their code.

### Breaking

- The `repos.path` field in config.json has been split into `source` and `storage_path`. `source` is now required and is meant to point to a URL or a folder to copy from. `storage_path` can be used to save the copy of the repo in a different place. (`storage_path` is unstable and might removed later on).

## [0.3.0] - 2023-10-28

### Breaking

- The `ROOT_DIR` has been changed from `$PWD` (the current folder) to `$HOME/.repotracer/`, so as to be stable no matter where the command is run. This means you should copy your config file, repos folder and stats folder to `$HOME/.repotracer`. Nobody currently uses this program but the author so this shouldn't be a burden.

### Changed

- Improved configuration system to use typed dataclasses instead of plain dictionaries with `dacite`.
- Added CI to run tests
- Added smoke test

## [0.2.5] - 2023-09-21

### Fixed

- Fixed a bad bug that was preventing running stats

## [0.2.4] - 2023-09-03

### Fixed

- Fixed a number of issues with repotracer add-repo
