use gix::Repository;
use std::fs;
use std::path::Path;

pub fn create_temp_repo(dir: &Path) -> Result<Repository, Box<dyn std::error::Error>> {
    fs::create_dir_all(dir)?;
    let repo = gix::init_bare(dir)?;
    Ok(repo)
}
