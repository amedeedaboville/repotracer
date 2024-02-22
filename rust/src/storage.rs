use crate::config::get_stats_dir;
use polars::prelude::*;
use std::error::Error;
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
    df: &mut DataFrame,
) -> Result<(), Box<dyn Error>> {
    let path = stat_storage_path(repo_name, stat_name);
    println!("Writing {repo_name}:{stat_name} to {}", path.display());
    let mut file = std::fs::File::create(&path).unwrap();
    CsvWriter::new(&mut file)
        .with_datetime_format(Some("%Y-%m-%d %H:%M:%S".into()))
        .finish(df)
        .unwrap();
    // wtr.flush()?;
    Ok(())
}

pub fn load_df(repo_name: &str, stat_name: &str) -> Result<DataFrame, Box<dyn Error>> {
    let file = std::fs::File::open(stat_storage_path(repo_name, stat_name)).unwrap();
    Ok(CsvReader::new(file).finish()?)
}
