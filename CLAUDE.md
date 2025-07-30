# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

Repotracer is a Rust CLI tool that collects statistics about git repositories over time. It iterates through repository history and measures various metrics (lines of code, regex matches, file counts, custom scripts) to track code changes and evolution patterns.

## Core Architecture

The project follows a modular architecture with clear separation of concerns:

### Key Components

- **Commands** (`src/commands/`): CLI command implementations including `run`, `add-stat`, `clone`, `config`, `serve`, and `detect-tools`
- **Stats/Measurements** (`src/stats/`): Different measurement types including:
  - `tokei`: Lines of code counting using the tokei library
  - `grep`: Regex pattern matching via ripgrep
  - `filecount`: File counting with glob patterns
  - `custom_script`: Execute arbitrary shell commands
  - `jq_collector`: JSON processing with jq queries
- **Collectors** (`src/collectors/`): Repository walking and data collection infrastructure
  - `cached_walker`: Core walker that processes git history with caching
  - `list_in_range`: Date range and granularity handling
- **Config** (`src/config.rs`): JSON5 configuration management for repos and stats
- **Storage** (`src/storage.rs`): Data persistence and CSV output generation

### Data Flow

1. User configures repositories and stats via JSON5 config or CLI commands
2. `CachedWalker` iterates through git commits at specified granularity (daily/weekly/monthly)
3. Each measurement type implements `FileMeasurement` trait to collect data at each commit
4. Results are cached and exported to CSV files with optional plotting

## Development Commands

### Building and Testing
```bash
cargo build          # Build the project
cargo build --release  # Release build
cargo check          # Quick syntax/type checking
cargo test           # Run tests
cargo clippy         # Lint checking
```

### Running the Tool
```bash
cargo run -- run --repo <repo_name> --stat <stat_name>  # Run specific stat
cargo run -- add-stat --repo <repo> --stat <stat>       # Interactive stat setup
cargo run -- config show                                # View configuration
cargo run -- detect-tools <repo_path>                   # Detect tools in repo
cargo run -- serve --port 8080                          # Start web interface
```

## Configuration

The tool uses JSON5 configuration files located at:
- `$PWD/.repotracer/config.json` (project-specific)
- `$HOME/.repotracer/config.json` (global)

Configuration structure includes repositories with associated statistics, where each stat defines its measurement type, parameters, and collection options.

## Key Dependencies

- **gix**: Modern Git library for repository operations
- **tokei**: Fast line counting across programming languages  
- **grep**: Text search functionality via ripgrep integration
- **serde**: JSON serialization for configuration and data export
- **chrono**: Date/time handling for historical analysis
- **clap**: Command-line argument parsing
- **rayon**: Parallel processing for performance
- **csv**: Data export functionality

## Extension Points

- **New Measurement Types**: Implement `FileMeasurement` trait in `src/stats/`
- **New Commands**: Add to `src/commands/` and wire up in `main.rs`
- **Custom Data Processing**: Extend collectors for different data aggregation patterns
- **Export Formats**: Modify storage layer for additional output formats beyond CSV

The codebase is designed for extensibility while maintaining performance through caching and parallel processing.