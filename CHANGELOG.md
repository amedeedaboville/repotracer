# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.2] - 2023-10-28
### Added
- New `loc_count` stat type powered by `tokei`. Can take a subset of languages to run for, and can graph/collect a language breakdown or the total

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
