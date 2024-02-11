use crate::stats::common::{FileMeasurement, NumMatches};
use anyhow::Error;
use gix::Repository;
use grep::matcher::Matcher;
use grep::regex::RegexMatcher;
use grep::searcher::sinks::UTF8;
use grep::searcher::Searcher;

pub struct RipgrepCollector {
    pattern: String,
}
impl RipgrepCollector {
    pub fn new(pattern: &str) -> Self {
        RipgrepCollector {
            pattern: pattern.to_string(),
        }
    }
    pub fn get_matches(
        &self,
        contents: &str,
    ) -> Result<Vec<(u64, String)>, Box<dyn std::error::Error>> {
        let matches = grep_slice(&self.pattern, contents.as_bytes())?;
        Ok(matches)
    }
}
impl FileMeasurement<NumMatches> for RipgrepCollector {
    fn measure_file(
        &self,
        repo: &Repository,
        path: &str,
        contents: &str,
    ) -> Result<NumMatches, Box<dyn std::error::Error>> {
        let matches = self.get_matches(contents)?;
        Ok(NumMatches(matches.len()))
    }
    fn measure_bytes(&self, contents: &[u8]) -> Result<NumMatches, Box<dyn std::error::Error>> {
        let matches = grep_slice(&self.pattern, contents)?;
        Ok(NumMatches(matches.len()))
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
