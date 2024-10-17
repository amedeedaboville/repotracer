use crate::collectors::cached_walker::CommitData;
use crate::config::get_stats_dir;
use csv::{Reader, Writer};
use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

fn stat_storage_path(repo_name: &str, stat_name: &str) -> PathBuf {
    let stats_dir = get_stats_dir().join(repo_name);
    if !stats_dir.exists() {
        println!("Creating stats dir at {}", stats_dir.display());
        std::fs::create_dir_all(&stats_dir).unwrap();
    }
    let filename = format!("{stat_name}.csv");
    stats_dir.join(filename)
}
pub fn write_commit_stats_to_csv(
    repo_name: &str,
    stat_name: &str,
    df: &Vec<CommitData>,
) -> Result<(), Box<dyn Error>> {
    let path = stat_storage_path(repo_name, stat_name);
    println!("Writing {repo_name}:{stat_name} to {}", path.display());

    let file = File::create(&path)?;
    let mut writer = Writer::from_writer(file);

    for commit in df {
        writer.serialize(commit)?;
    }

    writer.flush()?;
    Ok(())
}

pub fn load_df(repo_name: &str, stat_name: &str) -> Result<Vec<CommitData>, Box<dyn Error>> {
    let path = stat_storage_path(repo_name, stat_name);
    println!("Loading {repo_name}:{stat_name} from {}", path.display());

    let file = File::open(&path)?;
    let reader = BufReader::new(file);
    let mut csv_reader = Reader::from_reader(reader);

    let mut commits = Vec::new();
    for result in csv_reader.deserialize() {
        let commit: CommitData = result?;
        commits.push(commit);
    }

    Ok(commits)
}
