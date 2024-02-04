

use git2::{Repository, Oid, TreeWalkResult, TreeWalkMode};
use grep::searcher::sinks::UTF8;
use std::collections::HashMap;
use std::fs::File;
use std::os::macos::raw::stat;

use grep::matcher::{Matcher, NoCaptures};
use grep::searcher::{Searcher, Sink, SinkMatch};
use grep::regex::RegexMatcher;
use anyhow::Error;


fn walk_repo_and_collect_stats<T,L>(repo_path: &str, stat_collector: impl TreeCollector<T,L>) -> Result<(), git2::Error> {
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
            stat_collector.measure_either(&repo, &path, entry);

            // match tree_cache.get(&oid) {
            //     Some(stats) => { },
            //     None => {
            //         // If the tree is not cached, call the stat collection function and cache the result
            //         if let Some(blob) = entry.to_object(&repo).ok()?.as_blob() {
            //             let content = std::str::from_utf8(blob.content()).unwrap_or_default();
            //             let stats = stat_collection_fn(content);
            //             tree_cache.insert(oid, stats.clone());
            //             // Here you would process the stats as needed
            //         }
            //     }
            // }

            TreeWalkResult::Ok
        })?;
    }

    Ok(())
}

pub enum Either<L, R> {
    Left(L),
    Right(R),
}
type TreeDataCollection<Tree,Leaf> = HashMap<String, Either<Tree,Leaf>>;
trait TreeCollector<TreeData, LeafData>{
    fn measure_file(&self, repo: &Repository, path: &str, contents: &str) -> Result<LeafData, Box<dyn std::error::Error>>;
    fn collect(&self, repo: &Repository, child_data: TreeDataCollection<TreeData, LeafData>) -> Result<TreeData, Box<dyn std::error::Error>>;

    fn measure_either(&self, repo: &Repository, path: &str, entry: &git2::TreeEntry) -> Result<Either<TreeData, LeafData>, Box<dyn std::error::Error>> {
        match entry.kind() {
            Some(git2::ObjectType::Blob) => {
                let blob = entry.to_object(repo).unwrap().as_blob().unwrap();
                let content = std::str::from_utf8(blob.content()).unwrap_or_default();
                 self.measure_file(repo, path, content).map(Either::Right)
            },
            Some(git2::ObjectType::Tree) => {
                let tree = entry.to_object(&repo).unwrap().as_tree().unwrap();
                let child_results = tree.iter().map(|entry| {
                    let child_result = self.measure_either(repo, &format!("{}/", path), &entry).unwrap();
                    (entry.name().unwrap().to_string(), child_result)
                }).collect::<TreeDataCollection<TreeData, LeafData>>();
                self.collect(repo, child_results).map(Either::Left)
            },
            _ => Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "Unsupported git object type"))),
        }
    }
}

struct RipgrepCollector {
    pattern: String,
}
impl RipgrepCollector {
    pub fn new(pattern: &str) -> Self {
        RipgrepCollector {
            pattern: pattern.to_string(),
        }
    }
    fn measure_file(&self, repo: &Repository, path: &str, contents: &str) -> Result<NumMatches, Box<dyn std::error::Error>> {
        let matches = grep_slice(&self.pattern, contents)?;
        Ok(NumMatches(matches.len()))
    }
}

fn grep_slice(pattern: &str, contents: &str) -> Result<Vec<(u64, String)>, Error> {
        let matcher = RegexMatcher::new(pattern)?;
        let mut matches: Vec<(u64, String)> = vec![];
        Searcher::new().search_slice(&matcher, contents.as_bytes(), UTF8(|lnum, line| {
            // We are guaranteed to find a match, so the unwrap is OK.
            let mymatch = matcher.find(line.as_bytes())?.unwrap();
            matches.push((lnum, line[mymatch].to_string()));
            Ok(true)
        }))?;
        Ok(matches)
    }

struct FileCount(usize);
struct NumMatches(usize);
//Number of files that match the pattern
impl TreeCollector<FileCount, NumMatches> for RipgrepCollector {
    fn measure_file(&self, repo: &Repository, path: &str, contents: &str) -> Result<NumMatches, Box<dyn std::error::Error>> {
        self.measure_file(repo, path, contents)
    }

    fn collect(&self, repo: &Repository, child_data: TreeDataCollection<FileCount, NumMatches>) -> Result<FileCount, Box<dyn std::error::Error>> {
        let mut count = FileCount(0);
        for entry in child_data {
            count.0 += match entry {
                (_name, Either::Left(tree_val)) => tree_val.0,
                (_name, Either::Right(leaf)) => 1
            }
        }
        Ok(count)
    }
}

//Total number of matches in the repository
impl TreeCollector<NumMatches, NumMatches> for RipgrepCollector {
    fn measure_file(&self, repo: &Repository, path: &str, contents: &str) -> Result<NumMatches, Box<dyn std::error::Error>> {
        self.measure_file(repo, path, contents)
    }

    fn collect(&self, repo: &Repository, child_data: TreeDataCollection<NumMatches, NumMatches>) -> Result<NumMatches, Box<dyn std::error::Error>> {
        let mut count = NumMatches(0);
        for entry in child_data {
            count.0 += match entry {
                (_name, Either::Left(tree_val)) => tree_val.0,
                (_name, Either::Right(leaf)) => leaf.0
            }
        }
        Ok(count)
    }
}