

use git2::{Repository, Oid, TreeWalkResult, TreeWalkMode};
use std::collections::HashMap;


fn main() {

}

fn walk_repo_and_collect_stats(repo_path: &str, stat_collection_fn: &dyn Fn(&str) -> Value) -> Result<(), git2::Error> {
    let repo = Repository::open(repo_path)?;
    let head = repo.head()?.peel_to_commit()?;
    let mut tree_cache = HashMap::new();

    let mut revwalk = repo.revwalk()?;
    repo.revwalk()?.push_head()?;
    revwalk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::REVERSE)?;

    for oid in revwalk {
        let commit = repo.find_commit(oid?)?;
        let tree = commit.tree()?;

        // Walk through the tree of the commit
        tree.walk(TreeWalkMode::PreOrder, |root, entry| -> TreeWalkResult {
            let path = format!("{}{}", root, entry.name().unwrap_or_default());
            let oid = entry.id();

            match tree_cache.get(&oid) {
                Some(stats) => {
                    // Here you would use the cached stats instead of calling the stat collection function
                },
                None => {
                    // If the tree is not cached, call the stat collection function and cache the result
                    if let Some(blob) = entry.to_object(&repo).ok()?.as_blob() {
                        let content = std::str::from_utf8(blob.content()).unwrap_or_default();
                        let stats = stat_collection_fn(content);
                        tree_cache.insert(oid, stats.clone());
                        // Here you would process the stats as needed
                    }
                }
            }

            TreeWalkResult::Ok
        })?;
    }

    Ok(())
}