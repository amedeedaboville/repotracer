use std::path;

use crate::stats::common::{NumMatches, PathMeasurement};
use anyhow::Error;
use gix::Repository;
use globset::{Glob, GlobMatcher};

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

impl PathMeasurement<NumMatches> for PathBlobCollector {
    fn measure_path(
        &self,
        _repo: &Repository,
        path: &str,
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
}
