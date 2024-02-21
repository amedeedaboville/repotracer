use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::OnceCell;
use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Debug)]
struct StatConfig {
    name: String,
    description: String,
    #[serde(rename = "type")]
    type_: String,
    params: Value, // Using serde_json::Value to represent any JSON value
    path_in_repo: Option<String>,
    start: Option<String>,
    end: Option<String>,
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
    pub stat_storage: Option<HashMap<String, Value>>,
    pub repos: HashMap<String, RepoConfig>,
}
use std::sync::OnceLock;

static CONFIG: OnceLock<GlobalConfig> = OnceLock::new();

pub fn get_repo_config(repo: &str) -> &GlobalConfig {
    CONFIG.get_or_init(|| {
        let path = get_config_path();
        if !path.exists() {
            let root_dir = get_root_dir();
            if !root_dir.exists() {
                std::fs::create_dir_all(&root_dir).unwrap();
            }
            let default_config = GlobalConfig::new();
            default_config.write_to_file(&path).unwrap();
            return default_config;
        }
        GlobalConfig::read_from_file(&path).unwrap()
    })
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
            stat_storage: Some(HashMap::new()),
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

fn get_root_dir() -> PathBuf {
    env::current_dir().unwrap().join(".repotracer")
}

fn get_config_path() -> PathBuf {
    get_root_dir().join("config.json")
}
