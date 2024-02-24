use crate::stats::common::FileMeasurement;
use anyhow::Error;
use gix::{Repository, ThreadSafeRepository};
use polars::{
    datatypes::{DataType, Field},
    frame::row::Row,
    prelude::Schema,
};
use tokei::{CodeStats, Config, LanguageType, Report};

use super::common::{PossiblyEmpty, TreeDataCollection};

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
        if path.starts_with(".") {
            return Ok(CodeStats::new());
        }
        let language = LanguageType::from_path(path, &config)
            .ok_or_else(|| Error::msg(format!("Failed to get language type for path: '{path}'")))?;
        Ok(language.parse_from_slice(contents, &config))
    }

    fn summarize_tree_data(
        &self,
        tree_data: TreeDataCollection<CodeStats>,
    ) -> Result<Row, Box<dyn std::error::Error>> {
        let mut report = Report::new(std::path::PathBuf::from("."));
        for (filename, entry) in tree_data {
            report += entry;
        }
        //todo we have to pull out the Languages
        let total = (report.stats.code as u64).into();
        let row = Row::new(vec![total]); //todo pull out the the language totals
        Ok(row)
    }

    fn polars_schema(&self) -> Schema {
        let field = Field::new("total", DataType::UInt64);
        Schema::from_iter(vec![field])
    }
}

impl PossiblyEmpty for CodeStats {
    fn is_empty(&self) -> bool {
        self.lines() == 0
    }
}
