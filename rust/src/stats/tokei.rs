use std::ops::{Add, AddAssign};

use crate::stats::common::FileMeasurement;
use ahash::{HashMap, HashMapExt};
use anyhow::Error;
use gix::{Repository, ThreadSafeRepository};
use polars::{
    datatypes::{AnyValue, DataType, Field},
    frame::row::Row,
    prelude::Schema,
};
use tokei::{CodeStats, Config, Language, LanguageType, Languages, Report};

use super::common::{PossiblyEmpty, TreeDataCollection};

/// Imitates a tokei "Language" but simpler, bc I don't understand it.
/// Also derives Clone
#[derive(Clone, Debug)]
pub struct TokeiStat {
    pub language: LanguageType,
    /// The total number of blank lines.
    pub blanks: usize,
    /// The total number of lines of code.
    pub code: usize,
    /// The total number of comments(both single, and multi-line)
    pub comments: usize,
}
impl Default for TokeiStat {
    fn default() -> Self {
        Self {
            language: LanguageType::ABNF,
            blanks: 0,
            code: 0,
            comments: 0,
        }
    }
}
impl Add for TokeiStat {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            language: self.language,
            blanks: self.blanks + other.blanks,
            code: self.code + other.code,
            comments: self.comments + other.comments,
        }
    }
}
impl AddAssign for TokeiStat {
    fn add_assign(&mut self, other: Self) {
        self.blanks += other.blanks;
        self.code += other.code;
        self.comments += other.comments;
    }
}
//todo a list of languages we care about
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
impl FileMeasurement<TokeiStat> for TokeiCollector {
    fn measure_file(
        &self,
        _repo: &Repository,
        path: &str,
        contents: &str,
    ) -> Result<TokeiStat, Box<dyn std::error::Error>> {
        let config = Config::default();
        //tokei ignores dotfiles
        //todo we should add other paths to care about
        //todo we should make measure_file return an Result<Option>
        if path.starts_with(".") {
            return Ok(TokeiStat::default());
        }
        let language_type = LanguageType::from_path(path, &config)
            .ok_or_else(|| Error::msg(format!("Failed to get language type for path: '{path}'")))?;
        let codestat = language_type.parse_from_slice(contents, &config);
        let stat = TokeiStat {
            language: language_type,
            blanks: codestat.blanks,
            code: codestat.code,
            comments: codestat.comments,
        };
        Ok(stat)
    }

    fn summarize_tree_data(
        &self,
        tree_data: TreeDataCollection<TokeiStat>,
    ) -> Result<(Schema, Row), Box<dyn std::error::Error>> {
        let mut stats_by_language: HashMap<String, TokeiStat> = HashMap::new();
        for (_filename, stat) in tree_data.iter() {
            if stat.is_empty() {
                continue;
            }
            if stats_by_language.contains_key(&stat.language.to_string()) {
                let entry = stats_by_language
                    .get_mut(&stat.language.to_string())
                    .unwrap();
                *entry += stat.clone();
            } else {
                stats_by_language.insert(stat.language.to_string(), stat.clone());
            }
        }

        //group the tree_datas by language_type, and then + them all together
        //ignoring the path
        let mut languages: Vec<_> = stats_by_language
            .iter()
            .map(|(language, _stat)| language)
            .collect();

        languages.sort();
        let schema_languages = languages
            .iter()
            .map(|l| Field::new(&l.to_string(), DataType::UInt64));
        let schema = Schema::from_iter(schema_languages);

        let totals: Vec<AnyValue> = languages
            .iter()
            .map(|&l| (stats_by_language.get(l).unwrap().code as u64).into())
            .collect();

        let row = Row::new(totals); //todo pull out the the language totals
        Ok((schema, row))
    }
}

impl PossiblyEmpty for TokeiStat {
    fn is_empty(&self) -> bool {
        self.blanks == 0 && self.code == 0 && self.comments == 0
    }
}
