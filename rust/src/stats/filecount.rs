use crate::stats::common::{FileMeasurement, NumMatches};
use polars::{
    datatypes::{AnyValue, DataType, Field},
    frame::row::Row,
    prelude::Schema,
};

use gix::Repository;
use globset::{Glob, GlobMatcher};

use super::common::TreeDataCollection;

pub struct PathBlobCollector {
    glob: GlobMatcher,
}
impl PathBlobCollector {
    pub fn new(pattern: &str) -> Self {
        // let glob = Glob::new(pattern).expect("Failed to create glob");
        // let mut globset = GlobSetBuilder::new();
        // globset.add(glob);
        // let globset = globset.build().expect("Failed to build glob set");
        let glob = Glob::new(pattern)
            .expect("Failed to compile glob")
            .compile_matcher();

        PathBlobCollector { glob }
    }
}

impl FileMeasurement<NumMatches> for PathBlobCollector {
    fn measure_file(
        &self,
        _repo: &Repository,
        path: &str,
        _contents: &str,
    ) -> Result<NumMatches, Box<dyn std::error::Error>> {
        if path.ends_with(std::path::MAIN_SEPARATOR) {
            return Ok(NumMatches(0));
        }
        Ok(NumMatches(if self.glob.is_match(path) {
            // println!("{path} matches");
            1
        } else {
            0
        }))
    }
    fn summarize_tree_data(
        &self,
        data: TreeDataCollection<NumMatches>,
    ) -> Result<Row, Box<dyn std::error::Error>> {
        let total: u64 = data.into_iter().map(|(_, matches)| matches.0 as u64).sum();
        let val: AnyValue = total.into();
        let row = Row::new(vec![val]);
        Ok(row)
    }
    fn polars_schema(&self) -> Schema {
        let field = Field::new("total", DataType::UInt64);
        Schema::from_iter(vec![field])
    }
}
