use gix::{ObjectId, Repository, ThreadSafeRepository};
use polars::{frame::row::Row, prelude::Schema};
use std::{collections::BTreeMap, fmt::Display};

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
pub trait FileMeasurement<FileData>: Send + Sync
where
    FileData: Send + PossiblyEmpty,
{
    fn measure_file(
        &self,
        repo: &Repository,
        path: &str,
        contents: &str,
    ) -> Result<FileData, Box<dyn std::error::Error>>;

    fn measure_entry(
        &self,
        repo: &Repository,
        path: &str,
        oid: &ObjectId,
    ) -> Result<FileData, Box<dyn std::error::Error>> {
        let obj = repo.find_object(*oid)?;
        let content = std::str::from_utf8(&obj.data).unwrap_or_default();
        self.measure_file(repo, path, content)
    }

    fn summarize_tree_data(
        &self,
        child_data: TreeDataCollection<FileData>,
    ) -> Result<(Schema, Row), Box<dyn std::error::Error>>;
}
pub trait PossiblyEmpty {
    fn is_empty(&self) -> bool;
}
pub type TreeDataCollection<FileData> = BTreeMap<String, FileData>;

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
impl PossiblyEmpty for NumMatches {
    fn is_empty(&self) -> bool {
        self.0 == 0
    }
}
