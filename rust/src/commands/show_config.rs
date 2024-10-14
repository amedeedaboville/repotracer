use crate::config::get_config_path;

pub fn show_config_command() {
    let config_path = get_config_path();
    println!("Config file is located at: {}", config_path.display());
}
