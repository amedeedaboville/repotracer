use gix::{ObjectId, Repository};
use polars::{frame::row::Row, prelude::Schema};
use std::{collections::BTreeMap, fmt::Display};

use crate::collectors::repo_cache_data::AliasedPath;

#[derive(Debug, Clone, PartialEq)]
pub enum MeasurementKind {
    FilePathOnly,        // Only goes through the tree, doesn't open objects
    FilePathAndContents, //
    WholeRepo,           // not implemented. Will be slow
}
impl MeasurementKind {
    pub fn is_file_level(&self) -> bool {
        match self {
            MeasurementKind::FilePathOnly => true,
            MeasurementKind::FilePathAndContents => true,
            MeasurementKind::WholeRepo => false,
        }
    }
    pub fn can_batch(&self) -> bool {
        match self {
            MeasurementKind::FilePathOnly => false,
            MeasurementKind::FilePathAndContents => true,
            MeasurementKind::WholeRepo => false,
        }
    }
}

//todo we're not building any TreeDatas for now.
#[derive(Debug, Clone)]
pub enum MeasurementData<M>
where
    M:,
{
    //The usize contains the number of non-empty files in the children of the tree
    TreeData((usize, M)),
    FileData(M),
}
impl<M> MeasurementData<M>
where
    M: Display,
{
    pub fn to_string(&self) -> String {
        match self {
            MeasurementData::TreeData((_, l)) => l.to_string(),
            MeasurementData::FileData(r) => r.to_string(),
        }
    }
}
impl<M> MeasurementData<M> {
    pub fn unwrap_tree(self) -> M {
        match self {
            MeasurementData::TreeData((_, l)) => l,
            MeasurementData::FileData(_) => panic!("Called unwrap_tree on FileData"),
        }
    }
    pub fn unwrap_file(self) -> M {
        match self {
            MeasurementData::TreeData(_) => panic!("Called unwrap_file on TreeData"),
            MeasurementData::FileData(r) => r,
        }
    }
    pub fn get_count(&self) -> usize {
        match self {
            MeasurementData::TreeData((count, _)) => *count,
            MeasurementData::FileData(_) => 1,
        }
    }
}
pub trait FileData: Clone + Send + Sync + 'static {}
impl FileData for String {}
pub trait FileMeasurement: Sync {
    type Data: FileData;

    fn kind(&self) -> MeasurementKind;

    fn measure_file(
        &self,
        repo: &Repository,
        path: &str,
        contents: &str,
    ) -> Result<Self::Data, Box<dyn std::error::Error>>;

    fn measure_entry(
        &self,
        repo: &Repository,
        path: &str,
        oid: &ObjectId,
    ) -> Result<Self::Data, Box<dyn std::error::Error>> {
        if self.kind() == MeasurementKind::FilePathOnly {
            return self.measure_file(repo, path, "");
        }
        let obj = repo.find_object(*oid)?;
        let content = std::str::from_utf8(&obj.data).unwrap_or_default();
        self.measure_file(repo, path, content)
    }

    fn summarize_tree_data(
        &self,
        child_data: TreeDataCollection<Self::Data>,
    ) -> Result<(Schema, Row), Box<dyn std::error::Error>>;
}
pub trait PossiblyEmpty {
    fn is_empty(&self) -> bool;
}
impl PossiblyEmpty for String {
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
}
pub type TreeDataCollection<FileData> = BTreeMap<AliasedPath, FileData>;

#[derive(Debug, Clone, Copy)]
pub struct FileCount(pub usize);
unsafe impl Send for FileCount {}
unsafe impl Sync for FileCount {}
impl PossiblyEmpty for FileCount {
    fn is_empty(&self) -> bool {
        self.0 == 0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct NumMatches(pub usize);

unsafe impl Send for NumMatches {}
unsafe impl Sync for NumMatches {}
impl FileData for NumMatches {}
impl PossiblyEmpty for NumMatches {
    fn is_empty(&self) -> bool {
        self.0 == 0
    }
}

// pub fn str_to_measurement(name: &str) -> Result<FileMeasurement, String> {
//     match name {
//         "file" => Ok(Measurements::FileCount(FileCount(0))),
//         "grep" => Ok(Measurements::Grep(NumMatches(0))),
//         "Tokei" => Ok(Measurements::Tokei(TokeiStats::default())),
//         _ => Err(format!(
//             "{} is not a valid stat type. Known stat types are: 'FileCount', 'Grep', 'Tokei'",
//             name
//         )),
//     }
// }
