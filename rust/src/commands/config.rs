use crate::config;
use anyhow::Result;

pub fn config_location_command() {
    let config_path = config::get_config_path();
    println!("Config file location: {:?}", config_path);
}

pub fn config_show_command() -> Result<()> {
    let config = config::global_config();
    println!("{}", serde_json::to_string_pretty(&config)?);
    Ok(())
}

pub fn config_add_repo_command(_name: &str, _path: &str) -> Result<()> {
    println!("Not implemented yet");
    // let mut config = config::gloal_config();

    // let repo = config::UserRepoConfig {
    //     name: name.to_string(),
    //     path: path.to_string(),
    //     default_branch: "main".to_string(),
    // };

    // if let Some(repos) = config["repos"].as_object_mut() {
    //     repos.insert(name.to_string(), json!(path));
    //     write_config(&config)?;
    //     println!("Added repo '{}' with path '{}'", name, path);
    // } else {
    //     anyhow::bail!("Invalid config structure");
    // }

    Ok(())
}
