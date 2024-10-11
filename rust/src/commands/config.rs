use crate::config::get_config_path;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

fn read_config() -> Result<Value> {
    let config_path = get_config_path();
    let config_str = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read config file at {:?}", config_path))?;
    serde_json::from_str(&config_str).with_context(|| "Failed to parse config JSON")
}

fn write_config(config: &Value) -> Result<()> {
    let config_path = get_config_path();
    let config_str = serde_json::to_string_pretty(config)?;
    fs::write(&config_path, config_str)
        .with_context(|| format!("Failed to write config file at {:?}", config_path))?;
    Ok(())
}

pub fn config_location_command() {
    let config_path = get_config_path();
    println!("Config file location: {:?}", config_path);
}

pub fn config_show_command() -> Result<()> {
    let config = read_config()?;
    println!("{}", serde_json::to_string_pretty(&config)?);
    Ok(())
}

pub fn config_add_repo_command(name: &str, path: &str) -> Result<()> {
    let mut config = read_config().unwrap_or_else(|_| json!({"repos": {}}));

    if let Some(repos) = config["repos"].as_object_mut() {
        repos.insert(name.to_string(), json!(path));
        write_config(&config)?;
        println!("Added repo '{}' with path '{}'", name, path);
    } else {
        anyhow::bail!("Invalid config structure");
    }

    Ok(())
}
