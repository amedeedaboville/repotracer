use gix::{object::tree::EntryRef, oid, ObjectId, Repository};
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Display,
};

#[derive(Debug, Clone)]
pub enum Either<L, R> {
    Left(L),
    Right(R),
}
impl<L, R> Either<L, R>
where
    L: Display,
    R: Display,
{
    pub fn to_string(&self) -> String {
        match self {
            Either::Left(l) => l.to_string(),
            Either::Right(r) => r.to_string(),
        }
    }
}
impl<L, R> Either<L, R> {
    pub fn unwrap_left(self) -> L {
        match self {
            Either::Left(l) => l,
            Either::Right(_) => panic!("Called unwrap_left on Either::Right"),
        }
    }
    pub fn unwrap_right(self) -> R {
        match self {
            Either::Left(_) => panic!("Called unwrap_right on Either::Left"),
            Either::Right(r) => r,
        }
    }
}
pub trait FileMeasurement<T>: Sync
where
    T: Send,
{
    fn measure_file(
        &self,
        repo: &Repository,
        path: &str,
        contents: &str,
    ) -> Result<T, Box<dyn std::error::Error>>;

    fn measure_bytes(&self, contents: &[u8]) -> Result<T, Box<dyn std::error::Error>>;
}
pub trait PossiblyEmpty {
    fn is_empty(&self) -> bool;
}
pub trait TreeReducer<TreeData, FileData>
where
    TreeData: PossiblyEmpty,
{
    fn reduce(
        &self,
        repo: &Repository,
        child_data: TreeDataCollection<TreeData, FileData>,
    ) -> Result<TreeData, Box<dyn std::error::Error>>;
}
pub type TreeDataCollection<Tree, Leaf> = BTreeMap<String, Either<Tree, Leaf>>;

#[derive(Debug, Clone, Copy)]
pub struct FileCount(pub usize);
unsafe impl Send for FileCount {}
unsafe impl Sync for FileCount {}
impl PossiblyEmpty for FileCount {
    fn is_empty(&self) -> bool {
        self.0 == 0
    }
}
pub struct FileCountReducer {}
impl TreeReducer<FileCount, NumMatches> for FileCountReducer {
    fn reduce(
        &self,
        _repo: &Repository,
        child_data: TreeDataCollection<FileCount, NumMatches>,
    ) -> Result<FileCount, Box<dyn std::error::Error>> {
        let mut count = FileCount(0);
        for entry in child_data {
            count.0 +=
                match entry {
                    (_name, Either::Left(tree_val)) => tree_val.0,
                    (_name, Either::Right(leaf)) => {
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
pub struct NumMatchesReducer {}
impl TreeReducer<NumMatches, NumMatches> for NumMatchesReducer {
    fn reduce(
        &self,
        _repo: &Repository,
        child_data: TreeDataCollection<NumMatches, NumMatches>,
    ) -> Result<NumMatches, Box<dyn std::error::Error>> {
        let mut count = NumMatches(0);
        for entry in child_data {
            count.0 += match entry {
                (_name, Either::Left(tree_val)) => tree_val.0,
                (_name, Either::Right(leaf)) => leaf.0,
            }
        }
        Ok(count)
    }
}

pub trait PathMeasurement<T> {
    fn measure_path(&self, repo: &Repository, path: &str) -> Result<T, Box<dyn std::error::Error>>;
}

pub trait BlobMeasurer<F>: Sync
where
    F: Send + Sync,
{
    fn measure_entry(
        &self,
        repo: &Repository,
        path: &str,
        oid: &ObjectId,
    ) -> Result<F, Box<dyn std::error::Error>>;

    fn measure_data(&self, contents: &[u8]) -> Result<F, Box<dyn std::error::Error>>;
}

pub struct FileContentsMeasurer<F> {
    pub callback: Box<dyn FileMeasurement<F>>,
}
impl<F> BlobMeasurer<F> for FileContentsMeasurer<F>
where
    F: Send + Sync,
{
    fn measure_entry(
        &self,
        repo: &Repository,
        path: &str,
        oid: &ObjectId,
    ) -> Result<F, Box<dyn std::error::Error>> {
        let obj = repo.find_object(oid.clone())?;
        let content = std::str::from_utf8(&obj.data).unwrap_or_default();
        self.callback.measure_file(repo, path, content)
    }
    fn measure_data(&self, contents: &[u8]) -> Result<F, Box<dyn std::error::Error>> {
        self.callback.measure_bytes(contents)
    }
}
pub struct FilePathMeasurer<F> {
    pub callback: Box<dyn PathMeasurement<F> + Sync>, // Add + Sync here
}

impl<F> BlobMeasurer<F> for FilePathMeasurer<F>
where
    F: Send + Sync,
{
    fn measure_entry(
        &self,
        repo: &Repository,
        path: &str,
        oid: &ObjectId,
    ) -> Result<F, Box<dyn std::error::Error>> {
        self.callback.measure_path(repo, &path)
    }
    fn measure_data(&self, _contents: &[u8]) -> Result<F, Box<dyn std::error::Error>> {
        unimplemented!()
    }
}
