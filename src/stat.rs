use std::sync::Arc;

use chrono::{DateTime, Utc};

use crate::{
    collectors::{
        cached_walker::{CachedWalker, CommitData, MeasurementRunOptions},
        list_in_range::Granularity,
    },
    config::UserStatConfig,
    stats::{
        common::{FileMeasurement, NumMatches, NumberStat},
        custom_file_measurement::{CustomFileCommand, CustomFileMeasurement},
        custom_script::CustomScriptCollector,
        filecount::PathBlobCollector,
        grep::RipgrepCollector,
        jq_collector::{JqNumberCollector, JqNumberStat},
        tokei::{TokeiCollector, TokeiStat},
    },
};

pub enum FileDataEnum {
    TokeiStat(TokeiStat),
    NumMatches(NumMatches),
    String(String),
}
pub static ALL_MEASUREMENTS: [&str; 5] = [
    "tokei",
    "regex_count",
    "file_count",
    "script",
    "file_script",
];

pub enum Measurement {
    Tokei(Arc<dyn FileMeasurement<Data = TokeiStat> + Send + 'static>),
    Grep(Arc<dyn FileMeasurement<Data = NumMatches> + Send + 'static>),
    FileCount(Arc<dyn FileMeasurement<Data = NumMatches> + Send + 'static>),
    Script(Arc<dyn FileMeasurement<Data = String> + Send + 'static>),
    Jq(Arc<dyn FileMeasurement<Data = JqNumberStat> + Send + 'static>),
    FileScript(Arc<dyn FileMeasurement<Data = NumberStat> + Send + 'static>),
}
impl Measurement {
    pub fn run(
        &mut self,
        repo_path: String,
        granularity: Granularity,
        range: (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
        path_in_repo: Option<String>,
    ) -> Result<Vec<CommitData>, Box<dyn std::error::Error>> {
        //The walk_repo_and_collect_stats call is duplicated bc otherwise
        //if we had let walker = match self { ... } we'd get "match arms have incompatible types"
        //The walkers are different types, eg Walker<Tokei> vs Walker<NumMatches>.
        //I don't really know which way I want to have the types go yet, so this is fine for now
        //It's already kind of a mess, doing Walker<Box<dyn ...>> is even more of a mess, I just
        //want to get coding new stats for now.
        let options = MeasurementRunOptions {
            granularity,
            range,
            path_in_repo,
        };
        match self {
            Measurement::Tokei(tokei) => {
                let walker = CachedWalker::<TokeiStat>::new(repo_path, tokei.clone());
                let result = walker.walk_repo_and_collect_stats(options)?;
                std::mem::forget(walker);
                Ok(result)
            }
            Measurement::Grep(grep) => {
                let walker = CachedWalker::<NumMatches>::new(repo_path, grep.clone());
                let result = walker.walk_repo_and_collect_stats(options)?;
                std::mem::forget(walker);
                Ok(result)
            }
            Measurement::FileCount(filecount) => {
                let walker = CachedWalker::<NumMatches>::new(repo_path, filecount.clone());
                let result = walker.walk_repo_and_collect_stats(options)?;
                std::mem::forget(walker);
                Ok(result)
            }
            Measurement::Script(script) => {
                let walker = CachedWalker::<String>::new(repo_path, script.clone());
                let result = walker.walk_repo_and_collect_stats(options)?;
                std::mem::forget(walker);
                Ok(result)
            }
            Measurement::FileScript(file_script) => {
                let walker = CachedWalker::<NumberStat>::new(repo_path, file_script.clone());
                let result = walker.walk_repo_and_collect_stats(options)?;
                std::mem::forget(walker);
                Ok(result)
            }
            Measurement::Jq(jq) => {
                let walker = CachedWalker::<JqNumberStat>::new(repo_path, jq.clone());
                let result = walker.walk_repo_and_collect_stats(options)?;
                std::mem::forget(walker);
                Ok(result)
            }
        }
    }
}
pub fn build_measurement(config: &UserStatConfig) -> Measurement {
    let valid_measurements = ["tokei", "grep/regex_count", "file_count", "script"];
    match config.type_.as_str() {
        "tokei" => {
            let param_languages = config.get_param_array_string("languages");
            let param_top_n = config.get_param::<usize>("topn");
            let tokei_collector = TokeiCollector::new(param_languages, param_top_n);
            Measurement::Tokei(Arc::new(tokei_collector))
        }
        "grep" | "regex_count" => {
            let pattern = config
                .get_param("pattern")
                .unwrap_or_else(|| "todo".to_owned());
            Measurement::Grep(Arc::new(RipgrepCollector::new(pattern)))
        }
        "file_count" => {
            let param_path = config
                .get_param("pattern")
                .unwrap_or_else(|| "index.html".to_owned());
            Measurement::FileCount(Arc::new(PathBlobCollector::new(param_path)))
        }
        "script" => {
            let param_script = config
                .get_param("script")
                .unwrap_or_else(|| "echo 1".to_owned());
            Measurement::Script(Arc::new(CustomScriptCollector::new(param_script)))
        }
        "file_script" => {
            let param_command = config
                .get_param("command")
                .unwrap_or_else(|| "echo 1".to_owned());
            let param_args = config
                .get_param_array_string("args")
                .unwrap_or_else(|| vec![]);
            Measurement::FileScript(Arc::new(
                CustomFileMeasurement::new(CustomFileCommand {
                    executable: param_command,
                    args: param_args,
                })
                .expect("Failed to create CustomFileMeasurement"),
            ))
        }
        "jq" => {
            let param_jq = config.get_param("query").unwrap_or_else(|| ".".to_owned());
            Measurement::Jq(Arc::new(JqNumberCollector::new(&param_jq).unwrap()))
        }
        _ => panic!(
            "{} is not a valid stat type. Known stat types are: '{}'",
            config.type_,
            valid_measurements.join(", ")
        ),
    }
}
