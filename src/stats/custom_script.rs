use gix::Repository;

use super::common::{FileMeasurement, MeasurementKind, SummaryData};

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
        _repo: &Repository,
        _path: &str,
        _contents: &[u8],
    ) -> Result<Self::Data, Box<dyn std::error::Error>> {
        unimplemented!()
    }
    fn summarize_tree_data(
        &self,
        _child_data: &super::common::TreeDataCollection<Self::Data>,
    ) -> Result<SummaryData, Box<dyn std::error::Error>> {
        //Todo we have to figure out how to parse the output of the script.
        //We'll probably support reading a single number, a CSV line, or a JSON object.
        unimplemented!()
    }
}
