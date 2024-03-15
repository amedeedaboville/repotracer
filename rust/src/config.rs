use serde::{Deserialize, Serialize};
use serde_json::Value;

use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Debug)]
pub struct StatConfig {
    pub name: Option<String>,
    pub description: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub params: Value, // Using serde_json::Value to represent any JSON value
    pub path_in_repo: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RepoConfig {
    pub name: String,
    pub source: Option<String>,
    pub storage_path: Option<String>,
    pub default_branch: Option<String>,
    pub stats: Option<HashMap<String, StatConfig>>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GlobalConfig {
    pub repo_storage_location: Option<String>,
    pub stat_storage: Option<StatStorageConfig>,
    pub repos: HashMap<String, RepoConfig>,
}
#[derive(Serialize, Deserialize, Debug)]
pub struct StatStorageConfig {
    #[serde(rename = "type")]
    pub type_: String,
    pub storage_path: Option<String>,
}
use std::sync::OnceLock;

static GLOBAL_CONFIG: OnceLock<GlobalConfig> = OnceLock::new();

pub fn global_config() -> &'static GlobalConfig {
    GLOBAL_CONFIG.get_or_init(|| {
        let path = get_config_path();
        if !path.exists() {
            let root_dir = get_root_dir();
            if !root_dir.exists() {
                std::fs::create_dir_all(root_dir).unwrap();
            }
            let default_config = GlobalConfig::new();
            default_config.write_to_file(&path).unwrap();
            return default_config;
        }
        GlobalConfig::read_from_file(&path).unwrap()
    })
}
static ROOT_DIR: OnceLock<PathBuf> = OnceLock::new();
fn get_root_dir() -> &'static PathBuf {
    ROOT_DIR.get_or_init(|| env::current_dir().unwrap().join(".repotracer"))
}
pub fn get_repo_config(repo: &str) -> &RepoConfig {
    let global = global_config();
    global
        .repos
        .get(repo)
        .expect("We don't have a config for this repo")
}
pub fn get_stats_dir() -> PathBuf {
    global_config()
        .stat_storage
        .as_ref()
        .and_then(|s| s.storage_path.as_ref())
        .map(PathBuf::from)
        .unwrap_or_else(|| get_root_dir().join("stats"))
}
/*
const DEFAULT_REPOS: HashMap<String, RepoConfig> =
    vec![
        (
            "betterer",
            RepoConfig {
                name: "betterer".to_string(),
                source: None,
                storage_path: "~/clones/betterer".to_string(),
                default_branch: Some("master"),
            },
        ),
        (
            "linux",
            RepoConfig {
                name: "linux".to_string(),
                source: None,
                storage_path: "~/repos/github.com/torvalds/linux".to_string(),
                default_branch: Some("master"),
            },
        ),
        (
            "react",
            RepoConfig {
                name: "react".to_string(),
                source: None,
                storage_path: "~/repos/github.com/facebook/react".to_string(),
                default_branch: Some("master"),
            },
        ),
    ]
    .into_iter()
    .collect();

const DEFAULT_CONFIG: &str = r#"{
    "repo_storage_location": null,
    "stat_storage": {},
    "repos": {}
}"#;
*/
impl GlobalConfig {
    fn new() -> Self {
        Self {
            repo_storage_location: None,
            stat_storage: None,
            repos: HashMap::new(),
        }
    }

    fn read_from_file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        let config: Self = serde_json::from_str(&contents)?;
        Ok(config)
    }

    fn write_to_file<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        let contents = serde_json::to_string_pretty(self)?;
        let mut file = File::create(path)?;
        file.write_all(contents.as_bytes())?;
        Ok(())
    }
    // pub fn get_repo_config(repo_name: &str) -> Option<RepoConfig> {
    //     let config = Self::read_from_file(get_config_path()).unwrap();
    //     config.repos.get(repo).cloned()
    // }
}

fn get_config_path() -> PathBuf {
    get_root_dir().join("config.json")
}
