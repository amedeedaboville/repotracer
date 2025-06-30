pub mod commands {
    pub mod add_stat;
    pub mod clone;
    pub mod config;
    pub mod guess_tools;
    pub mod run;
    pub mod run_one_off;
    pub mod run_stat;
    pub mod serve;
}
pub mod stats {
    pub mod common;
    pub mod custom_file_measurement;
    pub mod custom_script;
    pub mod filecount;
    pub mod grep;
    pub mod jq_collector;
    pub mod test_utils;
    pub mod tokei;
}

pub mod collectors {
    pub mod cached_walker;
    pub mod list_in_range;
    pub mod repo_cache_data;
}

pub mod config;
pub mod repo;
pub mod stat;
pub mod storage;
pub mod util;
