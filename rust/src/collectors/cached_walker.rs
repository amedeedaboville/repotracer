use std::collections::HashMap;

use git2::Repository;

use std::fmt::Debug;

use crate::stats::common::{
    Either, FileMeasurement, PathMeasurement, TreeDataCollection, TreeReducer,
};

pub trait BlobMeasurer<F> {
    fn measure_entry(
        &self,
        repo: &Repository,
        path: &str,
        entry: &git2::TreeEntry,
    ) -> Result<F, Box<dyn std::error::Error>>;
}

pub struct FileContentsMeasurer<F> {
    pub callback: Box<dyn FileMeasurement<F>>,
}
impl<F> BlobMeasurer<F> for FileContentsMeasurer<F> {
    fn measure_entry(
        &self,
        repo: &Repository,
        _path: &str,
        entry: &git2::TreeEntry,
    ) -> Result<F, Box<dyn std::error::Error>> {
        let obj = entry.to_object(repo).unwrap();
        let blob = obj.as_blob().unwrap();
        let content = std::str::from_utf8(blob.content()).unwrap_or_default();
        self.callback.measure_file(repo, "", content)
    }
}
pub struct FilePathMeasurer<F> {
    pub callback: Box<dyn PathMeasurement<F>>,
}
impl<F> BlobMeasurer<F> for FilePathMeasurer<F> {
    fn measure_entry(
        &self,
        repo: &Repository,
        path: &str,
        entry: &git2::TreeEntry,
    ) -> Result<F, Box<dyn std::error::Error>> {
        self.callback.measure_path(repo, &path)
    }
}
pub struct CachedWalker<T, F> {
    cache: HashMap<git2::Oid, Either<T, F>>,
    repo: Repository,
    file_measurer: Box<dyn BlobMeasurer<F>>,
    tree_reducer: Box<dyn TreeReducer<T, F>>,
}
impl<T, F> CachedWalker<T, F>
where
    T: Debug + Clone,
    F: Debug + Clone,
{
    pub fn new(
        repo_path: &str,
        file_measurer: Box<dyn BlobMeasurer<F>>, // Changed type here
        tree_reducer: Box<dyn TreeReducer<T, F>>,
    ) -> Self {
        CachedWalker {
            cache: HashMap::new(),
            repo: Repository::open(repo_path).unwrap(),
            file_measurer,
            tree_reducer,
        }
    }
    pub fn walk_repo_and_collect_stats(&mut self) -> Result<(), git2::Error> {
        let inner_repo = Repository::open(self.repo.path())?;
        let mut revwalk = inner_repo.revwalk()?;
        revwalk.push_head()?;
        revwalk.set_sorting(git2::Sort::TOPOLOGICAL)?;
        revwalk.simplify_first_parent()?;
        let mut commit_stats = vec![];
        println!("Starting to walk the repo");

        let mut i = 0;
        for oid in revwalk {
            let Ok(oid) = oid else {
                let e = oid.unwrap_err();
                println!("Error with commit: {:?}", e);
                continue;
            };
            i += 1;
            let commit = inner_repo.find_commit(oid)?;
            let tree = commit.tree()?;
            let inner_repo = Repository::open(self.repo.path())?;
            let res = self.measure_either("", &tree, &inner_repo).unwrap();
            commit_stats.push((oid.to_string(), res));
        }
        println!("did: {i}");
        for (oid, stats) in commit_stats {
            println!("{oid}, {:?}", stats);
        }
        Ok(())
    }
    fn measure_either(
        &mut self,
        path: &str,
        tree: &git2::Tree,
        repo: &Repository,
    ) -> Result<Either<T, F>, Box<dyn std::error::Error>> {
        // println!("{}", object.id());
        if self.cache.contains_key(&tree.id()) {
            return Ok(self.cache.get(&tree.id()).unwrap().clone());
        }
        let res = {
            // let res = match object.kind() {
            // Some(git2::ObjectType::Blob) => self
            //     .file_measurer
            //     .measure_entry(&self.repo, path, &entry)
            //     .map(Either::Right),
            // Some(git2::ObjectType::Tree) => {
            // let tree = object.as_tree().unwrap();
            let child_results = tree
                .iter()
                .map(|entry| {
                    let entry_name = entry.name().unwrap().to_string();
                    if self.cache.contains_key(&entry.id()) {
                        return (entry_name, self.cache.get(&entry.id()).unwrap().clone());
                    }
                    match entry.kind() {
                        Some(git2::ObjectType::Tree) => {
                            let child_object = entry.to_object(&repo).unwrap();
                            let child_result = self
                                .measure_either(
                                    &format!("{path}/{}", entry.name().unwrap_or_default()),
                                    &child_object.as_tree().unwrap(),
                                    repo,
                                )
                                .unwrap();
                            (entry_name, child_result)
                        }
                        Some(git2::ObjectType::Blob) => {
                            let child_result = self
                                .file_measurer
                                .measure_entry(repo, path, &entry)
                                .map(Either::Right)
                                .unwrap();
                            (entry_name, child_result)
                        }
                        _ => panic!("Unsupported git object type"),
                    }
                })
                .collect::<TreeDataCollection<T, F>>();
            self.tree_reducer
                .reduce(&self.repo, child_results)
                .map(Either::Left)
        };
        // }
        // _ => bail!("Unsupported git object type"),
        //     _ => panic!("Unsupported git object type"),
        // };
        let res = res?;
        self.cache.insert(tree.id(), res.clone());
        Ok(res)
    }
}
