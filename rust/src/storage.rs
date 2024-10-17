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

    // Write header
    let mut header = vec!["commit", "date"];
    if let Some(first_commit) = df.first() {
        header.extend(first_commit.data.keys().map(|s| s.as_str()));
    }
    writer.write_record(&header)?;

    // Write data
    for commit in df {
        let oid_str = commit.oid.to_string();
        let date_str = commit.date.to_string();
        let mut record = vec![&oid_str, &date_str];
        let empty = String::new();
        for key in header.iter().skip(2) {
            let value = commit.data.get(*key).unwrap_or(&empty);
            record.push(value);
        }
        writer.write_record(&record)?;
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

    let headers: Vec<String> = csv_reader.headers()?.iter().map(String::from).collect();
    let mut commits = Vec::new();
    for result in csv_reader.deserialize() {
        let commit: CommitData = result?;
        commits.push(commit);
    }

    Ok(commits)
}
