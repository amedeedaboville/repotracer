use serde::{Deserialize, Serialize};
use serde_json::Value;

use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UserStatConfig {
    pub name: String,
    pub description: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub params: Option<Value>, // Using serde_json::Value to represent any JSON value
    pub path_in_repo: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub granularity: Option<Granularity>,
}

impl UserStatConfig {
    pub fn get_param_value(&self, name: &str) -> Option<&Value> {
        self.params.as_ref().and_then(|p| p.get(name))
    }
    pub fn get_param<T>(&self, name: &str) -> Option<T>
    where
        T: serde::de::DeserializeOwned,
    {
        self.params.as_ref().and_then(|t| {
            t.as_object().and_then(|p| p.get(name)).map(|v| {
                serde_json::from_value(v.clone())
                    .unwrap_or_else(|_| panic!("Failed to deserialize {name}"))
            })
        })
    }

    pub fn get_param_array_string(&self, name: &str) -> Option<Vec<String>> {
        self.get_param_value(name)
            .and_then(|l| l.as_array())
            .map(|l| {
                l.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
    }
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UserRepoConfig {
    pub name: String,
    pub source: String,
    pub storage_path: Option<String>,
    pub default_branch: Option<String>,
    pub stats: Option<HashMap<String, UserStatConfig>>,
}
impl UserRepoConfig {
    pub fn get_storage_path(&self) -> String {
        self.storage_path.clone().unwrap_or_else(|| {
            get_root_dir()
                .join("repos")
                .join(self.source.clone())
                .join(self.name.clone())
                .to_string_lossy()
                .to_string()
        })
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GlobalConfig {
    pub repo_storage_location: Option<String>,
    pub stat_storage: Option<StatStorageConfig>,
    pub repos: HashMap<String, UserRepoConfig>,
}
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StatStorageConfig {
    #[serde(rename = "type")]
    pub type_: String,
    pub storage_path: Option<String>,
}
use std::sync::OnceLock;

use crate::collectors::list_in_range::Granularity;

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
        GlobalConfig::read_from_file(&path)
            .unwrap_or_else(|_| panic!("Failed to read config file at {}", path.display()))
    })
}

static ROOT_DIR: OnceLock<PathBuf> = OnceLock::new();
fn get_root_dir() -> &'static PathBuf {
    ROOT_DIR.get_or_init(|| env::current_dir().unwrap().join(".repotracer"))
}
pub fn get_repo_config(repo: &str) -> Option<&UserRepoConfig> {
    global_config().repos.get(repo)
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
}

fn update_config(f: impl FnOnce(&mut GlobalConfig)) {
    let mut config = global_config().clone();
    f(&mut config);
    config.write_to_file(get_config_path()).unwrap();
}

pub fn add_repo(repo: UserRepoConfig) {
    update_config(|config| {
        config.repos.insert(repo.name.clone(), repo);
    });
}
pub fn add_stat(repo_name: &str, stat_name: &str, stat: UserStatConfig) {
    update_config(|config| {
        let repo = config.repos.get_mut(repo_name).expect("Repo not found");
        if repo.stats.is_none() {
            repo.stats = Some(HashMap::new());
        }
        repo.stats
            .as_mut()
            .unwrap()
            .insert(stat_name.to_string(), stat);
    });
}

pub fn list_repos() -> Vec<String> {
    global_config().repos.keys().cloned().collect()
}

pub fn list_stats(repo: &str) -> Option<Vec<String>> {
    global_config()
        .repos
        .get(repo)
        .and_then(|r| r.stats.as_ref().map(|s| s.keys().cloned().collect()))
}

pub fn get_stat_config(repo: &str, stat: &str) -> Option<&'static UserStatConfig> {
    global_config()
        .repos
        .get(repo)
        .and_then(|r| r.stats.as_ref().and_then(|s| s.get(stat)))
}

pub fn get_config_path() -> PathBuf {
    get_root_dir().join("config.json")
}
