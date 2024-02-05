use git2::{Repository, TreeWalkMode, TreeWalkResult};
use grep::searcher::sinks::UTF8;
use polars::frame::DataFrame;
use std::collections::HashMap;
use std::fmt::Debug;

use anyhow::Error;
use clap::{Arg, Command};
use grep::matcher::{Match, Matcher, NoCaptures};
use grep::regex::RegexMatcher;
use grep::searcher::{Searcher, Sink, SinkMatch};

struct Stat {
    name: String,
    data: DataFrame,
}
fn main() {
    let matches =
        Command::new("Repotracer")
            .version("0.1")
            .author("Amédée d'Aboville")
            .about("collects")
            .arg(
                Arg::new("repo_path")
                    .short('r')
                    .long("repo")
                    .value_name("REPO_PATH")
                    .default_value("/Users/amedee/clones/betterer")
                    .help("Sets the path to the repo to walk"),
            )
            .get_matches();
    let repo_path = matches.get_one::<String>("repo_path").unwrap();
    let mut walker = CachedWalker::new(
        repo_path,
        Box::new(RipgrepCollector::new("fn")),
        Box::new(RipgrepCollector::new("fn")),
    );
    walker.walk_repo_and_collect_stats();
}

struct CachedWalker<T, F> {
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
        let mut commit_stats = vec![];
        println!("Starting to walk the repo");

        let mut i = 0;
        for oid in revwalk {
            if let Err(e) = oid {
                println!("Error with commit: {:?}", e);
                continue;
            }
            i += 1;
            let oid = oid?;
            // println!("Walking commit {:?}", oid);
            let commit = inner_repo.find_commit(oid)?;
            let tree = commit.tree()?;

            // Walk through the tree of the commit
            tree.walk(TreeWalkMode::PreOrder, |root, entry| -> TreeWalkResult {
                let path = format!("{}{}", root, entry.name().unwrap_or_default());
                let r = self.measure_either(&path, entry);
                if let Ok(r) = r {
                    commit_stats.push(r.clone());
                }
                TreeWalkResult::Ok
            })?;
        }
        println!("{i}");
        println!("{:?}", commit_stats.len());
        Ok(())
    }
    fn measure_either<'a>(
        &'a mut self,
        path: &str,
        entry: &git2::TreeEntry,
    ) -> Result<Either<T, F>, Box<dyn std::error::Error>> {
        if self.cache.contains_key(&entry.id()) {
            return Ok(self.cache.get(&entry.id()).unwrap().clone());
        }
        let res = match entry.kind() {
            Some(git2::ObjectType::Blob) => {
                let object = entry.to_object(&self.repo).unwrap();
                let blob = object.as_blob().unwrap();
                let content = std::str::from_utf8(blob.content()).unwrap_or_default();
                self.file_measurer
                    .measure_file(&self.repo, path, content)
                    .map(Either::Right)
            }
            Some(git2::ObjectType::Tree) => {
                let inner_repo = Repository::open(self.repo.path())?;
                let object = entry.to_object(&inner_repo).unwrap(); // Create a longer lived value
                let tree = object.as_tree().unwrap();
                let child_results = tree
                    .iter()
                    .map(|entry| {
                        let child_result =
                            self.measure_either(&format!("{}/", path), &entry).unwrap();
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
        self.cache.insert(entry.id(), res.clone());
        Ok(res)
    }
}

#[derive(Debug, Clone)]
pub enum Either<L, R> {
    Left(L),
    Right(R),
}
trait FileMeasurement<T> {
    fn measure_file(
        &self,
        repo: &Repository,
        path: &str,
        contents: &str,
    ) -> Result<T, Box<dyn std::error::Error>>;
}
trait TreeReducer<TreeData, FileData> {
    fn reduce(
        &self,
        repo: &Repository,
        child_data: TreeDataCollection<TreeData, FileData>,
    ) -> Result<TreeData, Box<dyn std::error::Error>>;
}
type TreeDataCollection<Tree, Leaf> = HashMap<String, Either<Tree, Leaf>>;

// }

struct RipgrepCollector {
    pattern: String,
}
impl RipgrepCollector {
    pub fn new(pattern: &str) -> Self {
        RipgrepCollector {
            pattern: pattern.to_string(),
        }
    }
    fn get_matches(
        &self,
        repo: &Repository,
        path: &str,
        contents: &str,
    ) -> Result<Vec<(u64, String)>, Box<dyn std::error::Error>> {
        let matches = grep_slice(&self.pattern, contents)?;
        Ok(matches)
    }
}

fn grep_slice(pattern: &str, contents: &str) -> Result<Vec<(u64, String)>, Error> {
    let matcher = RegexMatcher::new(pattern)?;
    let mut matches: Vec<(u64, String)> = vec![];
    Searcher::new().search_slice(
        &matcher,
        contents.as_bytes(),
        UTF8(|lnum, line| {
            // We are guaranteed to find a match, so the unwrap is OK.
            let mymatch = matcher.find(line.as_bytes())?.unwrap();
            matches.push((lnum, line[mymatch].to_string()));
            Ok(true)
        }),
    )?;
    Ok(matches)
}

#[derive(Debug, Clone, Copy)]
struct FileCount(usize);
#[derive(Debug, Clone, Copy)]
struct NumMatches(usize);
//Number of files that match the pattern
impl TreeReducer<FileCount, NumMatches> for RipgrepCollector {
    fn reduce(
        &self,
        repo: &Repository,
        child_data: TreeDataCollection<FileCount, NumMatches>,
    ) -> Result<FileCount, Box<dyn std::error::Error>> {
        let mut count = FileCount(0);
        for entry in child_data {
            count.0 += match entry {
                (_name, Either::Left(tree_val)) => tree_val.0,
                (_name, Either::Right(leaf)) => 1,
            }
        }
        Ok(count)
    }
}

impl FileMeasurement<NumMatches> for RipgrepCollector {
    fn measure_file(
        &self,
        repo: &Repository,
        path: &str,
        contents: &str,
    ) -> Result<NumMatches, Box<dyn std::error::Error>> {
        let matches = self.get_matches(repo, path, contents)?;
        Ok(NumMatches(matches.len()))
    }
}
//Total number of matches in the repository
// impl TreeMeasurement<NumMatches, NumMatches> for RipgrepCollector {

//     fn reduce(&self, repo: &Repository, child_data: TreeDataCollection<NumMatches, NumMatches>) -> Result<NumMatches, Box<dyn std::error::Error>> {
//         let mut count = NumMatches(0);
//         for entry in child_data {
//             count.0 += match entry {
//                 (_name, Either::Left(tree_val)) => tree_val.0,
//                 (_name, Either::Right(leaf)) => leaf.0
//             }
//         }
//         Ok(count)
//     }
// }
