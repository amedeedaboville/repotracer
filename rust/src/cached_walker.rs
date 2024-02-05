use std::collections::HashMap;

use git2::{Repository, TreeWalkMode, TreeWalkResult};
use polars::frame::DataFrame;
use std::fmt::Debug;

use crate::stats::common::{Either, FileMeasurement, TreeDataCollection, TreeReducer};

pub struct CachedWalker<T, F> {
    cache: HashMap<git2::Oid, Either<T, F>>,
    repo: Repository,
    file_measurer: Box<dyn FileMeasurement<F>>,
    tree_reducer: Box<dyn TreeReducer<T, F>>,
}
impl<T, F> CachedWalker<T, F>
where
    T: Debug + Clone,
    F: Debug + Clone,
{
    pub fn new(
        repo_path: &str,
        file_measurer: Box<dyn FileMeasurement<F>>,
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
        // revwalk.simplify_first_parent()?;
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
            let res = self.measure_either("", tree.as_object()).unwrap();
            commit_stats.push((oid.to_string(), res));
        }
        println!("{i}");
        for (oid, stats) in commit_stats {
            println!("{oid}, {:?}", stats);
        }
        Ok(())
    }
    fn measure_either<'a>(
        &'a mut self,
        path: &str,
        object: &git2::Object,
    ) -> Result<Either<T, F>, Box<dyn std::error::Error>> {
        // println!("{}", object.id());
        if self.cache.contains_key(&object.id()) {
            return Ok(self.cache.get(&object.id()).unwrap().clone());
        }
        let res = match object.kind() {
            Some(git2::ObjectType::Blob) => {
                let blob = object.as_blob().unwrap();
                let content = std::str::from_utf8(blob.content()).unwrap_or_default();
                self.file_measurer
                    .measure_file(&self.repo, path, content)
                    .map(Either::Right)
            }
            Some(git2::ObjectType::Tree) => {
                let inner_repo = Repository::open(self.repo.path())?;
                let tree = object.as_tree().unwrap();
                let child_results = tree
                    .iter()
                    .map(|entry| {
                        // println!(
                        //     "doing child {}, {}",
                        //     entry.name().unwrap(),
                        //     entry.to_object(&inner_repo).unwrap().id()
                        // );
                        let child_object = entry.to_object(&inner_repo).unwrap();
                        let child_result = self
                            .measure_either(&format!("{}/", path), &child_object)
                            .unwrap();
                        (entry.name().unwrap().to_string(), child_result)
                    })
                    .collect::<TreeDataCollection<T, F>>();
                self.tree_reducer
                    .reduce(&self.repo, child_results)
                    .map(Either::Left)
            }
            // _ => bail!("Unsupported git object type"),
            _ => panic!("Unsupported git object type"),
        };
        let res = res?;
        self.cache.insert(object.id(), res.clone());
        Ok(res)
    }
}
