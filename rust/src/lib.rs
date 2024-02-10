pub mod commands {
    pub mod run;
}
pub mod stats {
    pub mod common;
    pub mod filecount;
    pub mod grep;
    pub mod tokei;
}

pub mod collectors {
    pub mod cached_walker;
}
