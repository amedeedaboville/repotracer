use crate::stats::common::{FileMeasurement, NumMatches};
use ahash::{HashMap, HashMapExt};
use anyhow::Error;
use gix::Repository;
use grep::matcher::Matcher;
use grep::regex::RegexMatcher;
use grep::searcher::sinks::UTF8;
use grep::searcher::Searcher;

use super::common::{MeasurementKind, SummaryData, TreeDataCollection};

pub struct RipgrepCollector {
    pattern: String,
}
impl RipgrepCollector {
    pub fn new(pattern: String) -> Self {
        RipgrepCollector { pattern }
    }
    pub fn get_matches(
        &self,
        contents: &str,
    ) -> Result<Vec<(u64, String)>, Box<dyn std::error::Error>> {
        let matches = grep_slice(&self.pattern, contents.as_bytes())?;
        Ok(matches)
    }
}
impl FileMeasurement for RipgrepCollector {
    type Data = NumMatches;
    fn kind(&self) -> MeasurementKind {
        MeasurementKind::FilePathAndContents
    }
    fn measure_file(
        &self,
        _repo: &Repository,
        _path: &str,
        contents: &str,
    ) -> Result<NumMatches, Box<dyn std::error::Error>> {
        let matches = self.get_matches(contents)?;
        Ok(NumMatches(matches.len()))
    }
    fn summarize_tree_data(
        &self,
        child_data: TreeDataCollection<NumMatches>,
    ) -> Result<SummaryData, Box<dyn std::error::Error>> {
        let total: u64 = child_data
            .into_values()
            .map(|matches| matches.0 as u64)
            .sum::<u64>();
        let mut data = HashMap::new();
        data.insert("total".to_string(), total.to_string());
        Ok(data)
    }
}

fn grep_slice(pattern: &str, contents: &[u8]) -> Result<Vec<(u64, String)>, Error> {
    let matcher = RegexMatcher::new(pattern)?;
    let mut matches: Vec<(u64, String)> = vec![];
    Searcher::new().search_slice(
        &matcher,
        contents,
        UTF8(|lnum, line| {
            // We are guaranteed to find a match, so the unwrap is OK.
            let mymatch = matcher.find(line.as_bytes())?.unwrap();
            matches.push((lnum, line[mymatch].to_string()));
            Ok(true)
        }),
    )?;
    Ok(matches)
}
