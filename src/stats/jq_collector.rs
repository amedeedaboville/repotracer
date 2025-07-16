use crate::stats::common::FileMeasurement;
use anyhow::Error;
use gix::Repository;
use jaq_core::{
    load::{Arena, File, Loader},
    Ctx, Native, RcIter,
};
use jaq_json::Val;

use super::common::{FileData, MeasurementKind, PossiblyEmpty, SummaryData, TreeDataCollection};
use ahash::{HashMap, HashMapExt};
use std::ops::{Add, AddAssign};

type Filter = jaq_core::Filter<Native<Val>>;

#[derive(Clone, Debug, PartialEq)]
pub struct JqNumberStat {
    pub count: f64,
}

impl FileData for JqNumberStat {}

impl Default for JqNumberStat {
    fn default() -> Self {
        Self { count: 0.0 }
    }
}

impl Add for JqNumberStat {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            count: self.count + other.count,
        }
    }
}

impl AddAssign for JqNumberStat {
    fn add_assign(&mut self, other: Self) {
        self.count += other.count;
    }
}

impl PossiblyEmpty for JqNumberStat {
    fn is_empty(&self) -> bool {
        self.count == 0.0
    }
}

pub struct JqNumberCollector {
    _query: String,
    compiled_query: Filter,
}

fn build_filter(query: &str) -> Result<Filter, Error> {
    let program = File {
        code: query,
        path: (),
    };
    let loader = Loader::new(jaq_std::defs().chain(jaq_json::defs()));
    let arena = Arena::default();
    let modules = loader
        .load(&arena, program)
        .map_err(|e| anyhow::anyhow!("Failed to load JQ (jaq) query '{}': {:?}", query, e))?;

    jaq_core::Compiler::default()
        .with_funs(jaq_std::funs().chain(jaq_json::funs()))
        .compile(modules)
        .map_err(|e| anyhow::anyhow!("Failed to compile JQ (jaq) query '{}': {:?}", query, e))
}

impl JqNumberCollector {
    pub fn new(query: &str) -> Result<Self, Error> {
        let filter = build_filter(query)?;
        Ok(Self {
            _query: query.to_string(),
            compiled_query: filter,
        })
    }
}

impl Default for JqNumberCollector {
    fn default() -> Self {
        Self::new(".dependencies | length")
            .expect("Failed to compile default JQ (jaq) query for JqNumberCollector")
    }
}

impl FileMeasurement for JqNumberCollector {
    type Data = JqNumberStat;

    fn kind(&self) -> MeasurementKind {
        MeasurementKind::FilePathAndContents
    }

    fn measure_file(
        &self,
        _repo: &Repository,
        path: &str,
        contents: &[u8],
    ) -> Result<JqNumberStat, Box<dyn std::error::Error>> {
        println!("[DEBUG JqNumberCollector] Processing file: {}", path);
        if !path.ends_with(".json") {
            return Ok(JqNumberStat::default());
        }
        if contents.is_empty() {
            println!(
                "[DEBUG JqNumberCollector] Skipping empty JSON file: {}",
                path
            );
            return Ok(JqNumberStat::default());
        }

        let serde_value = serde_json::from_slice::<serde_json::Value>(contents).map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse JSON from file {} with serde_json: {}. Content: {:#?}",
                path,
                e,
                contents
            )
        })?;
        let jaq_input_val = jaq_json::Val::from(serde_value);

        let inputs = RcIter::new(core::iter::empty());
        let ctx = Ctx::new([], &inputs);
        let mut results = self.compiled_query.run((ctx.clone(), jaq_input_val));

        if let Some(first_result) = results.next() {
            if results.next().is_some() {
                println!("[DEBUG JqNumberCollector] Query for {} produced more than one result. Returning default.", path);
                return Ok(JqNumberStat::default());
            }
            match first_result {
                Ok(val) => {
                    let val_str = val.to_string();
                    if let Ok(num_f64) = val_str.parse::<f64>() {
                        Ok(JqNumberStat { count: num_f64 })
                    } else {
                        println!("[DEBUG JqNumberCollector] Failed to parse JQ result as f64 for {}. Value (Val): {:?}, Stringified: '{}'", path, val, val_str);
                        Ok(JqNumberStat::default())
                    }
                }
                Err(e) => {
                    println!(
                        "[DEBUG JqNumberCollector] JQ execution error for {}: {:?}",
                        path, e
                    );
                    Ok(JqNumberStat::default())
                }
            }
        } else {
            println!(
                "[DEBUG JqNumberCollector] No result from JQ query for {}.",
                path
            );
            Ok(JqNumberStat::default())
        }
    }

    fn summarize_tree_data(
        &self,
        tree_data: &TreeDataCollection<JqNumberStat>,
    ) -> Result<SummaryData, Box<dyn std::error::Error>> {
        let mut total_count = 0.0;
        for (_filename, stat) in tree_data.iter() {
            if stat.is_empty() {
                continue;
            }
            total_count += stat.count;
        }

        let mut summary = HashMap::new();
        summary.insert("jq_result".to_string(), total_count.to_string());
        Ok(summary)
    }
}

/// Represents statistics collected from a JSON file where the JQ query returns an object
/// with string keys and number values.
#[derive(Clone, Debug, PartialEq)]
pub struct JqObjectStat {
    pub data: HashMap<String, usize>,
}

impl FileData for JqObjectStat {}

impl Default for JqObjectStat {
    fn default() -> Self {
        Self {
            data: HashMap::new(),
        }
    }
}

impl Add for JqObjectStat {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        let mut new_data = self.data.clone();
        for (key, value) in other.data {
            *new_data.entry(key).or_insert(0) += value;
        }
        Self { data: new_data }
    }
}

impl AddAssign for JqObjectStat {
    fn add_assign(&mut self, other: Self) {
        for (key, value) in other.data {
            *self.data.entry(key).or_insert(0) += value;
        }
    }
}

impl PossiblyEmpty for JqObjectStat {
    fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

pub struct JqObjectCollector {
    query: String,
    compiled_query: Filter,
}

impl JqObjectCollector {
    pub fn new(query: &str) -> Result<Self, Error> {
        let filter = build_filter(query)?;
        Ok(Self {
            query: query.to_string(),
            compiled_query: filter,
        })
    }
}
impl FileMeasurement for JqObjectCollector {
    type Data = JqObjectStat;

    fn kind(&self) -> MeasurementKind {
        MeasurementKind::FilePathAndContents
    }

    fn measure_file(
        &self,
        _repo: &Repository,
        path: &str,
        contents: &[u8],
    ) -> Result<JqObjectStat, Box<dyn std::error::Error>> {
        if !path.ends_with(".json") {
            return Ok(JqObjectStat::default());
        }

        let serde_value = serde_json::from_slice::<serde_json::Value>(contents).map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse JSON from file {} with serde_json: {}. Content: {:?}",
                path,
                e,
                contents
            )
        })?;
        let jaq_input_val = jaq_json::Val::from(serde_value);

        let inputs = RcIter::new(core::iter::empty());
        let ctx = Ctx::new([], &inputs);
        let mut results = self.compiled_query.run((ctx.clone(), jaq_input_val));
        if let Some(first_result) = results.next() {
            if results.next().is_some() {
                // Query produced more than one result, not expected for an object.
                return Ok(JqObjectStat::default());
            }
            match first_result {
                Ok(val) => match val {
                    Val::Obj(obj_map_rc) => {
                        // obj_map_rc is Rc<IndexMap<Rc<str>, Val>>
                        let mut data = HashMap::new();
                        for (key_rc, value_val) in obj_map_rc.iter() {
                            // key_rc: &Rc<str>, value_val: &Val
                            let key = key_rc.to_string();

                            if let Ok(num_isize) = value_val.to_string().parse::<isize>() {
                                if num_isize >= 0 {
                                    data.insert(key, num_isize as usize);
                                }
                            } else if let Ok(num_f64) = value_val.to_string().parse::<f64>() {
                                if num_f64 >= 0.0
                                    && num_f64.fract() == 0.0
                                    && num_f64 <= usize::MAX as f64
                                {
                                    data.insert(key, num_f64 as usize);
                                }
                            }
                            // Non-numeric or unsuitable numeric values in the object are ignored
                        }
                        Ok(JqObjectStat { data })
                    }
                    _ => Ok(JqObjectStat::default()), // Not an object
                },
                Err(_) => Ok(JqObjectStat::default()), // JQ (jaq) execution error
            }
        } else {
            Ok(JqObjectStat::default()) // No result from JQ (jaq) query
        }
    }

    fn summarize_tree_data(
        &self,
        tree_data: &TreeDataCollection<JqObjectStat>,
    ) -> Result<SummaryData, Box<dyn std::error::Error>> {
        let mut aggregated_data: HashMap<String, usize> = HashMap::new();
        for (_filename, stat) in tree_data.iter() {
            if stat.is_empty() {
                continue;
            }
            for (key, value) in &stat.data {
                *aggregated_data.entry(key.clone()).or_insert(0) += *value;
            }
        }

        let mut summary = HashMap::new();
        for (key, value) in aggregated_data {
            summary.insert(
                format!("jq_obj_query_{}_{}", self.query, key), // Key format kept
                value.to_string(),
            );
        }
        Ok(summary)
    }
}
