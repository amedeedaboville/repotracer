use crate::stats::common::FileMeasurement;
use anyhow::Error;
use gix::{Repository, ThreadSafeRepository};
use tokei::{CodeStats, Config, LanguageType, Report};

use super::common::{MeasurementData, PossiblyEmpty, ReduceFrom, TreeDataCollection};

pub struct TokeiCollector {}

impl Default for TokeiCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl TokeiCollector {
    pub fn new() -> Self {
        TokeiCollector {}
    }
}
impl FileMeasurement<CodeStats> for TokeiCollector {
    fn measure_file(
        &self,
        _repo: &Repository,
        path: &str,
        contents: &str,
    ) -> Result<CodeStats, Box<dyn std::error::Error>> {
        let config = Config::default();
        let language = LanguageType::from_path(path, &config)
            .ok_or_else(|| Error::msg(format!("Failed to get language type for path: '{path}'")))?;
        Ok(language.parse_from_slice(contents, &config))
    }
}

pub struct TokeiReducer {}
impl PossiblyEmpty for CodeStats {
    fn is_empty(&self) -> bool {
        self.lines() == 0
    }
}
impl PossiblyEmpty for Report {
    fn is_empty(&self) -> bool {
        self.stats.is_empty()
    }
}
impl ReduceFrom<CodeStats> for Report {
    fn reduce(
        _repo: &ThreadSafeRepository,
        child_data: TreeDataCollection<Report, CodeStats>,
    ) -> Result<Report, Box<dyn std::error::Error>> {
        let mut report = Report::new(std::path::PathBuf::from("."));
        for entry in child_data {
            report += match entry {
                (_name, MeasurementData::TreeData(report)) => report.stats,
                (_name, MeasurementData::FileData(code_stats)) => code_stats,
            }
        }
        Ok(report)
    }
}
