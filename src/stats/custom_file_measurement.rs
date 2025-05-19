use std::env;
use std::fs::File;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use ahash::{HashMap, HashMapExt};
use gix::Repository;

use super::common::{
    FileMeasurement, MeasurementKind, NumberStat, SummaryData, TreeDataCollection,
};

#[derive(Debug, Clone)]
pub struct CustomFileCommand {
    pub executable: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CustomFileMeasurement {
    command: CustomFileCommand,
}

impl CustomFileMeasurement {
    pub fn new(command: CustomFileCommand) -> Result<Self, String> {
        if command.executable.is_empty() {
            return Err("Command executable cannot be empty".to_string());
        }
        Ok(CustomFileMeasurement { command })
    }
}

impl FileMeasurement for CustomFileMeasurement {
    type Data = NumberStat; // Output of the command is now expected to be a number

    fn kind(&self) -> MeasurementKind {
        MeasurementKind::FilePathAndContents
    }

    fn measure_file(
        &self,
        _repo: &Repository,
        _original_path: &str, // Path in repo, can be used for context if needed
        contents: &str,
    ) -> Result<Self::Data, Box<dyn std::error::Error>> {
        let temp_file_name = format!(
            "repotracer_custom_{}_{}.tmp",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| format!("System time error: {}", e))?
                .as_nanos()
        );
        let mut temp_file_path = env::temp_dir();
        temp_file_path.push(temp_file_name);

        {
            let mut file = File::create(&temp_file_path)?;
            file.write_all(contents.as_bytes())?;
        }

        let mut cmd = Command::new(&self.command.executable);
        cmd.args(&self.command.args);
        cmd.arg(&temp_file_path);
        cmd.stdin(Stdio::null());

        let output = cmd.output()?;

        let _ = std::fs::remove_file(&temp_file_path); // Attempt cleanup

        if output.status.success() {
            let stdout_str = String::from_utf8(output.stdout)
                .map_err(|e| format!("Command output was not valid UTF-8: {}", e))?;

            // Try to parse the output as f64.
            // Assumes the command outputs a number, possibly with leading/trailing whitespace
            // or other text (like filename from wc -l).
            let parts: Vec<&str> = stdout_str.trim().split_whitespace().collect();
            if parts.is_empty() {
                return Err(format!(
                    "Command output was empty or only whitespace: '{}'",
                    stdout_str
                )
                .into());
            }

            let number = parts[0].parse::<f64>().map_err(|e| {
                format!(
                    "Failed to parse command output '{}' (from full output '{}') as f64: {}",
                    parts[0],
                    stdout_str.trim(),
                    e
                )
            })?;
            Ok(NumberStat(number))
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!(
                "Command `{} {} {}` failed with status {}: {}",
                self.command.executable,
                self.command.args.join(" "),
                temp_file_path.to_string_lossy(),
                output.status,
                stderr
            )
            .into())
        }
    }

    fn summarize_tree_data(
        &self,
        child_data: &TreeDataCollection<Self::Data>,
    ) -> Result<SummaryData, Box<dyn std::error::Error>> {
        let total: f64 = child_data.values().map(|number| number.0 as f64).sum();
        let mut data = HashMap::new();
        data.insert("result".to_string(), total.to_string());
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::test_utils::create_temp_repo;
    use tempfile::tempdir;

    #[test]
    fn test_measure_file_wc_l() -> Result<(), Box<dyn std::error::Error>> {
        let repo = create_temp_repo(tempdir()?.path())?;
        let measurement = CustomFileMeasurement::new(CustomFileCommand {
            executable: "wc".to_string(),
            args: vec!["-l".to_string()],
        })?;
        let five_line_content = "line one\nline two\nline three\nline four\nline five\n";
        let result_f64 = measurement.measure_file(&repo, "test_file.txt", five_line_content)?;

        assert_eq!(result_f64.0, 5.0, "Expected measure_file to return 5.0");

        Ok(())
    }
}
