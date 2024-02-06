use crate::stats::common::{Either, PathMeasurement, TreeDataCollection, TreeReducer};
use git2::Repository;
use std::collections::BTreeMap;
use std::fmt::Debug;

// Walks a git repository and collects stats about which files are present in a commit.
// Somewhat equivalent to (git rev-list HEAD| xargs -n1 -I{} sh -c 'git ls-tree -r {}  | $yourmeasurement | wc -l'

pub struct TreeWalker<T, F> {
    cache: BTreeMap<git2::Oid, Either<T, F>>,
    repo: Repository,
    tree_measurer: Box<dyn PathMeasurement<F>>,
    tree_reducer: Box<dyn TreeReducer<T, F>>,
}
impl<T, F> TreeWalker<T, F>
where
    T: Debug + Clone,
    F: Debug + Clone,
{
    pub fn new(
        repo_path: &str,
        tree_measurer: Box<dyn PathMeasurement<F>>,
        tree_reducer: Box<dyn TreeReducer<T, F>>,
    ) -> Self {
        TreeWalker {
            cache: BTreeMap::new(),
            repo: Repository::open(repo_path).unwrap(),
            tree_measurer,
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
            let res = self
                .measure_either("", tree.as_object(), &inner_repo)
                .unwrap();
            if oid == git2::Oid::from_str("ad5bc61a7319f2ab1f212e7711049632059a67e5").unwrap() {
                println!("tree id {:?}", tree.id());
                println!("{:?}", res);
                println!(
                    "{:?}",
                    self.cache.get(
                        &git2::Oid::from_str("1ec6d4f929de5b7558671d151d2bf3df9de49c92").unwrap(),
                    ),
                );
            }
            commit_stats.push((oid.to_string(), res));
        }
        println!("{i}");
        for (oid, stats) in commit_stats {
            // println!("{oid}, {:?}", stats);
        }
        Ok(())
    }
    fn measure_either(
        &mut self,
        path: &str,
        object: &git2::Object,
        repo: &Repository,
    ) -> Result<Either<T, F>, Box<dyn std::error::Error>> {
        // println!("{}", object.id());
        if self.cache.contains_key(&object.id()) {
            return Ok(self.cache.get(&object.id()).unwrap().clone());
        }
        let res = match object.kind() {
            Some(git2::ObjectType::Blob) => self
                .tree_measurer
                .measure_path(&self.repo, path)
                .map(Either::Right),
            Some(git2::ObjectType::Tree) => {
                let tree = object.as_tree().unwrap();
                let child_results = tree
                    .iter()
                    .map(|entry| {
                        if self.cache.contains_key(&entry.id()) {
                            return (
                                entry.name().unwrap().to_string(),
                                self.cache.get(&entry.id()).unwrap().clone(),
                            );
                        }
                        let child_object = entry.to_object(&repo).unwrap();
                        let child_result = self
                            .measure_either(
                                &format!("{path}/{}", entry.name().unwrap_or_default()),
                                &child_object,
                                repo,
                            )
                            .unwrap();
                        (entry.name().unwrap().to_string(), child_result)
                    })
                    .collect::<TreeDataCollection<T, F>>();
                if object.id()
                    == git2::Oid::from_str("0d28a401a957e7457baf22ddaee18eae785df2a5").unwrap()
                {
                    // println!("child results{:?}", child_results)
                }
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
