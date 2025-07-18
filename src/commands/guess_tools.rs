use anyhow::{Context, Result};
use globset::GlobMatcher;
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
    matcher: GlobMatcher,
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
                for tool_details in detected_tools_details {
                    let json_line = serde_json::to_string(&tool_details).with_context(|| {
                        format!("Failed to serialize tool details for {}", tool_details.name)
                    })?;
                    println!("{}", json_line);
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

fn compile_tool_definitions<R: BufRead>(reader: R) -> Result<Vec<CompiledToolRule>> {
    let mut rules = Vec::new();

    for (line_num, line_result) in reader.lines().enumerate() {
        let line = line_result.with_context(|| {
            format!("Failed to read line {} from tool definitions", line_num + 1)
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
                let glob = globset::Glob::new(p_str).with_context(|| {
                    format!("Invalid glob pattern '{}' for tool '{}'", p_str, def.name)
                })?;
                let matcher = glob.compile_matcher();
                compiled_file_patterns.push(CompiledPattern {
                    matcher,
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
                let glob = globset::Glob::new(p_str).with_context(|| {
                    format!("Invalid glob pattern '{}' for tool '{}'", p_str, def.name)
                })?;
                let matcher = glob.compile_matcher();
                compiled_dir_patterns.push(CompiledPattern {
                    matcher,
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

fn load_and_compile_tool_definitions(file_path: &Path) -> Result<Vec<CompiledToolRule>> {
    let file = File::open(file_path)
        .with_context(|| format!("Failed to open tool definitions file: {:?}", file_path))?;
    let reader = BufReader::new(file);
    compile_tool_definitions(reader)
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
                    let matches = if c_pattern.is_wildcard {
                        // For wildcard patterns, use glob matching on the full path
                        c_pattern.matcher.is_match(repo_file)
                    } else if c_pattern.original_pattern.starts_with('/') {
                        // For patterns starting with /, match against the full path from root
                        repo_file == &c_pattern.original_pattern[1..] // Remove leading slash
                    } else {
                        // For plain patterns, match against just the filename
                        let filename = std::path::Path::new(repo_file)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("");
                        filename == c_pattern.original_pattern
                    };

                    if matches {
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
                    let matches = if c_pattern.is_wildcard {
                        // For wildcard patterns, use glob matching on the full path
                        c_pattern.matcher.is_match(repo_file)
                    } else if c_pattern.original_pattern.starts_with('/') {
                        // For patterns starting with /, match files that start with the path from root
                        repo_file.starts_with(&c_pattern.original_pattern[1..]) // Remove leading slash
                    } else {
                        // For plain directory names, check if it appears as a path component
                        std::path::Path::new(repo_file)
                            .components()
                            .any(|component| {
                                if let std::path::Component::Normal(name) = component {
                                    name.to_str().unwrap_or("") == c_pattern.original_pattern
                                } else {
                                    false
                                }
                            })
                    };

                    if matches {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn create_test_tool_rules(ndjson: &str) -> Vec<CompiledToolRule> {
        let reader = Cursor::new(ndjson);
        compile_tool_definitions(reader).expect("Failed to compile test tool definitions")
    }

    #[derive(Debug)]
    struct MatchingTestCase {
        name: &'static str,
        tool_ndjson: &'static str,
        repo_files: Vec<&'static str>,
        expected_tool_count: usize,
        expected_tool_name: &'static str,
        expected_matching_files: Vec<&'static str>,
        expected_matching_dirs: Vec<&'static str>,
    }

    #[test]
    fn test_file_and_dir_matching() {
        let test_cases = vec![
            MatchingTestCase {
                name: "file_matching_basename_only",
                tool_ndjson: r#"{"name": "Make", "files": ["Makefile"], "dirs": []}"#,
                repo_files: vec![
                    "Makefile",
                    "drivers/Makefile",
                    "fs/ext4/Makefile",
                    "README.md",
                ],
                expected_tool_count: 1,
                expected_tool_name: "Make",
                expected_matching_files: vec!["Makefile", "drivers/Makefile", "fs/ext4/Makefile"],
                expected_matching_dirs: vec![],
            },
            MatchingTestCase {
                name: "file_matching_root_anchored",
                tool_ndjson: r#"{"name": "Git", "files": ["/.gitignore"], "dirs": []}"#,
                repo_files: vec![".gitignore", "subdir/.gitignore", "README.md"],
                expected_tool_count: 1,
                expected_tool_name: "Git",
                expected_matching_files: vec![".gitignore"],
                expected_matching_dirs: vec![],
            },
            MatchingTestCase {
                name: "file_matching_wildcard",
                tool_ndjson: r#"{"name": "CSharp", "files": ["*.csproj"], "dirs": []}"#,
                repo_files: vec![
                    "MyApp.csproj",
                    "src/MyLib.csproj",
                    "csproj/shouldntmatch.txt",
                    "alsocsproj/shouldntmatch.txt",
                    "README.md",
                ],
                expected_tool_count: 1,
                expected_tool_name: "CSharp",
                expected_matching_files: vec!["MyApp.csproj", "src/MyLib.csproj"],
                expected_matching_dirs: vec![],
            },
            MatchingTestCase {
                name: "dir_matching_basename_only",
                tool_ndjson: r#"{"name": "CDK", "files": [], "dirs": ["cdk"]}"#,
                repo_files: vec![
                    "cdk/app.py",
                    "infrastructure/cdk/stack.py",
                    "packages/cdk/lib.py",
                    "src/main.py",
                ],
                expected_tool_count: 1,
                expected_tool_name: "CDK",
                expected_matching_files: vec![],
                expected_matching_dirs: vec!["cdk"],
            },
            MatchingTestCase {
                name: "dir_matching_root_anchored",
                tool_ndjson: r#"{"name": "GitHub", "files": [], "dirs": ["/.github"]}"#,
                repo_files: vec![
                    ".github/workflows/ci.yml",
                    ".github/ISSUE_TEMPLATE.md",
                    "subproject/.github/workflows/test.yml",
                    "README.md",
                ],
                expected_tool_count: 1,
                expected_tool_name: "GitHub",
                expected_matching_files: vec![],
                expected_matching_dirs: vec!["/.github"],
            },
            MatchingTestCase {
                name: "dir_matching_wildcard",
                tool_ndjson: r#"{"name": "TestDirs", "files": [], "dirs": ["test*"]}"#,
                repo_files: vec![
                    "test/unit.py",
                    "src/test/integration.py",
                    "packages/lib/test/spec.py",
                    "src/main.py",
                ],
                expected_tool_count: 1,
                expected_tool_name: "TestDirs",
                expected_matching_files: vec![],
                expected_matching_dirs: vec!["test*"],
            },
            MatchingTestCase {
                name: "mixed_patterns",
                tool_ndjson: r#"{"name": "Mixed", "files": ["Cargo.toml", "/.gitignore", "*.rs"], "dirs": ["src", "/.github"]}"#,
                repo_files: vec![
                    "Cargo.toml",
                    "workspace/Cargo.toml",
                    ".gitignore",
                    "subdir/.gitignore",
                    "src/main.rs",
                    "lib/utils.rs",
                    "src/lib.rs",
                    ".github/workflows/ci.yml",
                    "subproject/.github/workflows/test.yml",
                ],
                expected_tool_count: 1,
                expected_tool_name: "Mixed",
                expected_matching_files: vec![
                    "Cargo.toml",
                    "workspace/Cargo.toml",
                    ".gitignore",
                    "src/main.rs",
                    "lib/utils.rs",
                    "src/lib.rs",
                ],
                expected_matching_dirs: vec!["src", "/.github"],
            },
            MatchingTestCase {
                name: "no_matches",
                tool_ndjson: r#"{"name": "NoMatch", "files": ["nonexistent.txt"], "dirs": ["nonexistent"]}"#,
                repo_files: vec!["README.md", "src/main.py"],
                expected_tool_count: 0,
                expected_tool_name: "",
                expected_matching_files: vec![],
                expected_matching_dirs: vec![],
            },
        ];

        for test_case in test_cases {
            println!("Running test case: {}", test_case.name);
            let rules = create_test_tool_rules(test_case.tool_ndjson);
            let repo_files: Vec<String> =
                test_case.repo_files.iter().map(|s| s.to_string()).collect();
            let results = detect_tools_with_details(&repo_files, &rules);

            assert_eq!(
                results.len(),
                test_case.expected_tool_count,
                "Test case '{}': Expected {} tools, got {}",
                test_case.name,
                test_case.expected_tool_count,
                results.len()
            );

            if test_case.expected_tool_count > 0 {
                let result = &results[0];
                assert_eq!(
                    result.name, test_case.expected_tool_name,
                    "Test case '{}': Expected tool name '{}', got '{}'",
                    test_case.name, test_case.expected_tool_name, result.name
                );

                // Check matching files
                for expected_file in &test_case.expected_matching_files {
                    assert!(
                        result
                            .matching_files_from_file_rules
                            .contains(&expected_file.to_string()),
                        "Test case '{}': Expected matching file '{}' not found in {:?}",
                        test_case.name,
                        expected_file,
                        result.matching_files_from_file_rules
                    );
                }

                // Check that we don't have extra files (for negative cases)
                for actual_file in &result.matching_files_from_file_rules {
                    assert!(
                        test_case
                            .expected_matching_files
                            .contains(&actual_file.as_str()),
                        "Test case '{}': Unexpected matching file '{}' found",
                        test_case.name,
                        actual_file
                    );
                }

                // Check matching directories
                for expected_dir in &test_case.expected_matching_dirs {
                    assert!(
                        result
                            .matched_dir_patterns
                            .contains(&expected_dir.to_string()),
                        "Test case '{}': Expected matching dir '{}' not found in {:?}",
                        test_case.name,
                        expected_dir,
                        result.matched_dir_patterns
                    );
                }

                // Check that we don't have extra directories
                for actual_dir in &result.matched_dir_patterns {
                    assert!(
                        test_case
                            .expected_matching_dirs
                            .contains(&actual_dir.as_str()),
                        "Test case '{}': Unexpected matching dir '{}' found",
                        test_case.name,
                        actual_dir
                    );
                }
            }
        }
    }

    #[test]
    fn test_wildcard_limit() {
        let ndjson = r#"{"name": "ManyFiles", "files": ["*.txt"], "dirs": []}"#;
        let rules = create_test_tool_rules(ndjson);

        // Create more files than MAX_WILDCARD_MATCHES
        let mut repo_files = Vec::new();
        for i in 0..100 {
            repo_files.push(format!("file{}.txt", i));
        }

        let results = detect_tools_with_details(&repo_files, &rules);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "ManyFiles");
        assert_eq!(
            results[0].matching_files_from_file_rules.len(),
            MAX_WILDCARD_MATCHES
        );
    }
}
