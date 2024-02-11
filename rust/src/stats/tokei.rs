use crate::stats::common::{FileMeasurement, NumMatches};
use anyhow::Error;
use gix::Repository;
use tokei::{CodeStats, Config, Language, LanguageType, Report};

use super::common::{Either, TreeDataCollection, TreeReducer};

pub struct TokeiCollector {}

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

    fn measure_bytes(&self, _contents: &[u8]) -> Result<CodeStats, Box<dyn std::error::Error>> {
        unimplemented!()
    }
}

pub struct TokeiReducer {}
impl TreeReducer<Report, CodeStats> for TokeiReducer {
    fn reduce(
        &self,
        _repo: &Repository,
        child_data: TreeDataCollection<Report, CodeStats>,
    ) -> Result<Report, Box<dyn std::error::Error>> {
        let mut report = Report::new(std::path::PathBuf::from("."));
        for entry in child_data {
            report += match entry {
                (_name, Either::Left(report)) => report.stats,
                (_name, Either::Right(code_stats)) => code_stats,
            }
        }
        Ok(report)
    }
}
