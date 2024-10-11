pub mod commands {
    pub mod clone;
    pub mod config;
    pub mod run;
    pub mod show_config;
}
pub mod stats {
    pub mod common;
    pub mod custom_script;
    pub mod filecount;
    pub mod grep;
    pub mod tokei;
}

pub mod collectors {
    pub mod cached_walker;
    pub mod list_in_range;
    pub mod repo_cache_data;
}

pub mod config;
pub mod plotter;
pub mod polars_utils;
pub mod stat;
pub mod storage;
pub mod util;
