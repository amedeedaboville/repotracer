use std::process::Command;

use gix::Repository;

use super::common::{FileMeasurement, MeasurementKind};

pub struct CustomScriptCollector {
    pub script: String,
}

impl CustomScriptCollector {
    pub fn new(script: String) -> Self {
        Self { script }
    }
}
impl FileMeasurement for CustomScriptCollector {
    type Data = String;

    fn kind(&self) -> MeasurementKind {
        MeasurementKind::WholeRepo
    }
    fn measure_file(
        &self,
        repo: &Repository,
        path: &str,
        contents: &str,
    ) -> Result<Self::Data, Box<dyn std::error::Error>> {
        unimplemented!()
    }
    fn summarize_tree_data(
        &self,
        child_data: super::common::TreeDataCollection<Self::Data>,
    ) -> Result<(polars::prelude::Schema, polars::frame::row::Row), Box<dyn std::error::Error>>
    {
        //Todo we have to figure out how to parse the output of the script.
        //We'll probably support reading a single number, a CSV line, or a JSON object.
        unimplemented!()
    }
}
