use gix::{ObjectId, Repository, ThreadSafeRepository};
use std::{collections::BTreeMap, fmt::Display};

#[derive(Debug, Clone)]
pub enum MeasurementData<L, R> {
    TreeData(L),
    FileData(R),
}
impl<L, R> MeasurementData<L, R>
where
    L: Display,
    R: Display,
{
    pub fn to_string(&self) -> String {
        match self {
            MeasurementData::TreeData(l) => l.to_string(),
            MeasurementData::FileData(r) => r.to_string(),
        }
    }
}
impl<L, R> MeasurementData<L, R> {
    pub fn unwrap_tree(self) -> L {
        match self {
            MeasurementData::TreeData(l) => l,
            MeasurementData::FileData(_) => panic!("Called unwrap_left on Either::Right"),
        }
    }
    pub fn unwrap_file(self) -> R {
        match self {
            MeasurementData::TreeData(_) => panic!("Called unwrap_right on Either::Left"),
            MeasurementData::FileData(r) => r,
        }
    }
}
// File level measurements measure the contents of a file.
// Then when we aggregate file-level results we sometimes store them
// in a different type of TreeData. This trait allows us to
// reduce the file-level measurements into the tree-level measurements.
pub trait ReduceFrom<T>
where
    Self: Sized,
{
    fn reduce(
        repo: &ThreadSafeRepository,
        child_data: TreeDataCollection<Self, T>,
    ) -> Result<Self, Box<dyn std::error::Error>>;
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
}
pub trait PossiblyEmpty {
    fn is_empty(&self) -> bool;
}
pub type TreeDataCollection<Tree, Leaf> = BTreeMap<String, MeasurementData<Tree, Leaf>>;

#[derive(Debug, Clone, Copy)]
pub struct FileCount(pub usize);
unsafe impl Send for FileCount {}
unsafe impl Sync for FileCount {}
impl PossiblyEmpty for FileCount {
    fn is_empty(&self) -> bool {
        self.0 == 0
    }
}
impl ReduceFrom<NumMatches> for FileCount {
    fn reduce(
        _repo: &ThreadSafeRepository,
        child_data: TreeDataCollection<FileCount, NumMatches>,
    ) -> Result<FileCount, Box<dyn std::error::Error>> {
        let mut count = FileCount(0);
        for entry in child_data {
            count.0 += match entry {
                (_name, MeasurementData::TreeData(tree_val)) => tree_val.0,
                (_name, MeasurementData::FileData(leaf)) => {
                    if leaf.0 > 0 {
                        1
                    } else {
                        0
                    }
                }
            }
        }
        Ok(count)
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
impl ReduceFrom<NumMatches> for NumMatches {
    fn reduce(
        _repo: &ThreadSafeRepository,
        child_data: TreeDataCollection<NumMatches, NumMatches>,
    ) -> Result<NumMatches, Box<dyn std::error::Error>> {
        let mut count = NumMatches(0);
        for entry in child_data {
            count.0 += match entry {
                (_name, MeasurementData::TreeData(tree_val)) => tree_val.0,
                (_name, MeasurementData::FileData(leaf)) => leaf.0,
            }
        }
        Ok(count)
    }
}
