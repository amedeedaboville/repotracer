use anyhow::{Context, Result};
use globset::Glob;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use walkdir::WalkDir;

const MAX_WILDCARD_MATCHES: usize = 10;

#[derive(Deserialize, Debug, Clone)]
struct ToolDefinition {
    name: String,
    files: Option<Vec<String>>,
    dirs: Option<Vec<String>>,
}

#[derive(Debug)]
struct CompiledPattern {
    glob: Glob,
    original_pattern: String,
    is_wildcard: bool,
}

#[derive(Debug)]
struct CompiledToolRule {
    name: String,
    file_patterns: Vec<CompiledPattern>,
    dir_patterns: Vec<CompiledPattern>,
}

#[derive(Serialize, Debug, Default, Clone)]
struct ToolMatchDetails {
    name: String,
    #[serde(rename = "matching_files")]
    matching_files_from_file_rules: Vec<String>,
    #[serde(rename = "matching_dirs")]
    matched_dir_patterns: Vec<String>,
}

#[derive(Serialize, Debug)]
struct ScannedRepoToolMatch {
    repo_path: String,
    tool_name: String,
    #[serde(rename = "matching_files")]
    matching_files_from_file_rules: Vec<String>,
    #[serde(rename = "matching_dirs")]
    matched_dir_patterns: Vec<String>,
}

pub fn detect_tools_command(
    tool_definitions: &PathBuf,
    repo_path: Option<PathBuf>,
    json: bool,
    scan_root: Option<PathBuf>,
    max_depth: usize,
) -> Result<()> {
    let tool_rules = Arc::new(
        load_and_compile_tool_definitions(&tool_definitions)
            .with_context(|| "Failed to load and compile tool definitions")?,
    );

    if let Some(scan_root_path) = scan_root {
        // Multi-repository scan mode (always outputs NDJSON)
        eprintln!(
            "Scanning for repositories under '{:?}' up to depth {}. Outputting NDJSON.",
            scan_root_path, max_depth
        );

        for entry in WalkDir::new(scan_root_path.clone())
            .min_depth(max_depth) // Start at the target depth
            .max_depth(max_depth) // Only consider this exact depth
            .into_iter()
            .filter_map(Result::ok) // Ignore errors during walk (e.g. permission denied)
            .filter(|e| e.file_type().is_dir())
        {
            let current_repo_path = entry.path();
            eprintln!(
                "Attempting to process potential repository: {:?}",
                current_repo_path
            );

            match list_git_files(current_repo_path) {
                Ok(repo_files) => {
                    if repo_files.is_empty() {
                        eprintln!(
                            "No files found or not a git repo (or empty): {:?}",
                            current_repo_path
                        );
                        continue;
                    }
                    eprintln!(
                        "Found {} files in {:?}",
                        repo_files.len(),
                        current_repo_path
                    );
                    let detected_tools_details =
                        detect_tools_with_details(&repo_files, &tool_rules);

                    for details in detected_tools_details {
                        let ndjson_entry = ScannedRepoToolMatch {
                            repo_path: current_repo_path.to_string_lossy().into_owned(),
                            tool_name: details.name,
                            matching_files_from_file_rules: details.matching_files_from_file_rules,
                            matched_dir_patterns: details.matched_dir_patterns,
                        };
                        let json_line =
                            serde_json::to_string(&ndjson_entry).with_context(|| {
                                format!(
                                    "Failed to serialize NDJSON entry for {:?}",
                                    current_repo_path
                                )
                            })?;
                        println!("{}", json_line);
                    }
                }
                Err(e) => {
                    eprintln!("Skipping directory {:?}: not a valid git repository or error listing files: {}", current_repo_path, e);
                }
            }
        }
        eprintln!("Multi-repository scan complete.");
    } else if let Some(single_repo_path) = repo_path {
        // Single repository mode
        eprintln!("Processing single repository: {:?}", single_repo_path);
        let repo_files = list_git_files(&single_repo_path).with_context(|| {
            format!(
                "Failed to list files in git repository: {:?}",
                single_repo_path
            )
        })?;

        eprintln!(
            "Loaded {} tool rules and {} files from repository {:?}.",
            tool_rules.len(),
            repo_files.len(),
            single_repo_path
        );

        let detected_tools_details: Vec<ToolMatchDetails> =
            detect_tools_with_details(&repo_files, &tool_rules);

        if json {
            let json_output = serde_json::to_string(&detected_tools_details)
                .with_context(|| "Failed to serialize results to JSON")?;
            println!("{}", json_output);
        } else {
            if detected_tools_details.is_empty() {
                println!("No tools detected.");
            } else {
                println!("Detected tools:");
                let mut sorted_tool_names: Vec<String> = detected_tools_details
                    .iter()
                    .map(|t| t.name.clone())
                    .collect();
                sorted_tool_names.sort_unstable();
                for name in sorted_tool_names {
                    println!("- {}", name);
                }
            }
        }
    } else {
        // This case should ideally be prevented by clap's `required_unless_present`
        eprintln!(
            "Error: No repository path provided and not in scan mode. Use -r or --scan-root."
        );
        std::process::exit(1);
    }

    Ok(())
}

fn load_and_compile_tool_definitions(file_path: &Path) -> Result<Vec<CompiledToolRule>> {
    let file = File::open(file_path)
        .with_context(|| format!("Failed to open tool definitions file: {:?}", file_path))?;
    let reader = BufReader::new(file);
    let mut rules = Vec::new();

    for (line_num, line_result) in reader.lines().enumerate() {
        let line = line_result.with_context(|| {
            format!(
                "Failed to read line {} from tool definitions file",
                line_num + 1
            )
        })?;
        if line.trim().is_empty() {
            continue;
        }

        let def: ToolDefinition = serde_json::from_str(&line).with_context(|| {
            format!("Failed to parse JSON from line {}: {}", line_num + 1, line)
        })?;

        let mut compiled_file_patterns = Vec::new();
        if let Some(patterns) = &def.files {
            for p_str in patterns {
                if p_str.is_empty() {
                    continue;
                }
                let is_wildcard = p_str.contains('*');
                let glob = Glob::new(p_str).with_context(|| {
                    format!("Invalid glob pattern '{}' for tool '{}'", p_str, def.name)
                })?;
                compiled_file_patterns.push(CompiledPattern {
                    glob,
                    original_pattern: p_str.clone(),
                    is_wildcard,
                });
            }
        }

        let mut compiled_dir_patterns = Vec::new();
        if let Some(patterns) = &def.dirs {
            for p_str in patterns {
                if p_str.is_empty() {
                    continue;
                }
                let is_wildcard = p_str.contains('*');
                let glob = Glob::new(p_str).with_context(|| {
                    format!("Invalid glob pattern '{}' for tool '{}'", p_str, def.name)
                })?;
                compiled_dir_patterns.push(CompiledPattern {
                    glob,
                    original_pattern: p_str.clone(),
                    is_wildcard,
                });
            }
        }
        rules.push(CompiledToolRule {
            name: def.name.clone(),
            file_patterns: compiled_file_patterns,
            dir_patterns: compiled_dir_patterns,
        });
    }
    Ok(rules)
}

fn list_git_files(repo_path: &Path) -> Result<Vec<String>> {
    let repo = gix::discover(repo_path)
        .with_context(|| format!("Failed to discover git repository at {:?}", repo_path))?;
    let index = repo
        .index_or_empty()
        .with_context(|| "Failed to open repository index. Ensure it's a valid git repo.")?;
    let mut files = Vec::with_capacity(index.entries().len());
    for entry in index.entries() {
        files.push(String::from_utf8_lossy(entry.path(&index)).into_owned());
    }
    Ok(files)
}

fn detect_tools_with_details(
    repo_files: &[String],
    tool_rules: &[CompiledToolRule],
) -> Vec<ToolMatchDetails> {
    tool_rules
        .par_iter()
        .map(|rule| {
            let mut current_tool_matches = ToolMatchDetails {
                name: rule.name.clone(),
                ..Default::default()
            };

            for c_pattern in &rule.file_patterns {
                let mut matches_for_this_specific_file_pattern = Vec::new();
                for repo_file in repo_files {
                    if repo_file.contains(c_pattern.glob.regex()) {
                        if c_pattern.is_wildcard
                            && matches_for_this_specific_file_pattern.len() >= MAX_WILDCARD_MATCHES
                        {
                            break;
                        }
                        matches_for_this_specific_file_pattern.push(repo_file.clone());
                    }
                }
                current_tool_matches
                    .matching_files_from_file_rules
                    .extend(matches_for_this_specific_file_pattern);
            }

            for c_pattern in &rule.dir_patterns {
                let mut matched_this_dir_rule = false;
                for repo_file in repo_files {
                    if repo_file.contains(c_pattern.glob.regex()) {
                        matched_this_dir_rule = true;
                        break;
                    }
                }
                if matched_this_dir_rule {
                    current_tool_matches
                        .matched_dir_patterns
                        .push(c_pattern.original_pattern.clone());
                }
            }
            current_tool_matches
        })
        .filter(|details| {
            !details.matching_files_from_file_rules.is_empty()
                || !details.matched_dir_patterns.is_empty()
        })
        .map(|mut details| {
            details.matching_files_from_file_rules.sort_unstable();
            details.matching_files_from_file_rules.dedup();

            details.matched_dir_patterns.sort_unstable();
            details.matched_dir_patterns.dedup();
            details
        })
        .collect()
}
