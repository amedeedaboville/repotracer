use std::ops::{Add, AddAssign};

use crate::stats::common::FileMeasurement;
use ahash::{HashMap, HashMapExt};
use gix::Repository;
use tokei::{Config, LanguageType};

use super::common::{FileData, MeasurementKind, PossiblyEmpty, SummaryData, TreeDataCollection};

/// Imitates a tokei "Language" but simpler, bc I don't understand it.
/// Also derives Clone
#[derive(Clone, Debug, PartialEq)]
pub struct TokeiStat {
    pub language: LanguageType,
    /// The total number of blank lines.
    pub blanks: usize,
    /// The total number of lines of code.
    pub code: usize,
    /// The total number of comments(both single, and multi-line)
    pub comments: usize,
}
impl FileData for TokeiStat {}
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
pub struct TokeiCollector {
    languages: Option<Vec<LanguageType>>,
    top_n: Option<usize>,
    failed_extensions: Vec<String>,
    tokei_config: Config,
}
impl Default for TokeiCollector {
    fn default() -> Self {
        Self::new(None, None)
    }
}

fn str_to_language_type(name: &str) -> Result<LanguageType, serde_json::Error> {
    serde_json::from_value(serde_json::Value::String(name.to_string()))
}
impl TokeiCollector {
    pub fn new(languages: Option<Vec<String>>, top_n: Option<usize>) -> Self {
        TokeiCollector {
            languages: languages.map(|l| {
                l.into_iter()
                    .map(|l| {
                        match str_to_language_type(&l) {
                            Ok(lt) => Ok(lt),
                            Err(_) => Err(anyhow::anyhow!("Unsupported language: {}", l)),
                        }
                        .unwrap()
                    })
                    .collect()
            }),
            top_n,
            failed_extensions: Vec::new(),
            tokei_config: Config {
                treat_doc_strings_as_comments: Some(true),
                ..Config::default()
            },
        }
    }
}
impl FileMeasurement for TokeiCollector {
    type Data = TokeiStat;
    fn kind(&self) -> MeasurementKind {
        MeasurementKind::FilePathAndContents
    }
    fn measure_file(
        &self,
        _repo: &Repository,
        path: &str,
        contents: &[u8],
    ) -> Result<TokeiStat, Box<dyn std::error::Error>> {
        //tokei ignores dotfiles
        //todo we should add other paths to care about
        //todo we should make measure_file return an Result<Option>
        if path.starts_with('.') || path.contains("/.") {
            return Ok(TokeiStat::default());
        }

        let language_type = if let Some(lt) = LanguageType::from_path(path, &self.tokei_config) {
            lt
        } else {
            //For now ignore "failed to get language" errors, later we can log them to something
            // self.failed_extensions.push(path.to_string());
            return Ok(TokeiStat::default()); // Return early if None
        };
        if let Some(languages) = &self.languages {
            if !languages.contains(&language_type) {
                return Ok(TokeiStat::default());
            }
        }
        let codestat = language_type.parse_from_slice(contents, &self.tokei_config);
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
        tree_data: &TreeDataCollection<TokeiStat>,
    ) -> Result<SummaryData, Box<dyn std::error::Error>> {
        let mut stats_by_language: HashMap<LanguageType, TokeiStat> = HashMap::new();
        for (_filename, stat) in tree_data.iter() {
            if stat.is_empty() {
                continue;
            }
            stats_by_language
                .entry(stat.language)
                .and_modify(|e| *e += stat.clone())
                .or_insert(stat.clone());
        }

        // Sort languages by LOC in descending order
        let mut languages: Vec<_> = stats_by_language.into_iter().collect();
        languages.sort_by(|a, b| b.1.code.cmp(&a.1.code));

        // Apply top_n filter if set
        if let Some(top_n) = self.top_n {
            languages.truncate(top_n);
        }

        let mut data = HashMap::new();
        languages.iter().for_each(|(l, s)| {
            data.insert(l.to_string(), s.code.to_string());
        });
        Ok(data)
    }
}

impl PossiblyEmpty for TokeiStat {
    fn is_empty(&self) -> bool {
        self.blanks == 0 && self.code == 0 && self.comments == 0
    }
}
