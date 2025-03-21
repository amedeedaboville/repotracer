use crate::config::{self, UserStatConfig};
use crate::stat::ALL_MEASUREMENTS;
use dialoguer::console::{style, Style};
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Input, Select};

pub fn add_stat_command(repo_name: Option<&String>, stat_name: Option<&String>) {
    let mut my_theme = ColorfulTheme::default();
    my_theme.active_item_style = Style::new().for_stderr().black().on_white();
    my_theme.active_item_prefix = style("»".to_string()).for_stderr(); //.bold().green();

    let repo_name = match repo_name {
        Some(name) => name.to_string(),
        None => {
            let repos = config::list_repos();
            let selection = Select::with_theme(&my_theme)
                .with_prompt("Which repo do you want to add a new stat for?")
                .items(&repos)
                .default(0)
                .interact()
                .unwrap();
            repos[selection].to_string()
        }
    };

    let repo_config = match config::get_repo_config(&repo_name) {
        Some(config) => config,
        None => {
            println!("Repo '{}' not found. Please add the repo first.", repo_name);
            return;
        }
    };

    let stat_name = match stat_name {
        Some(name) => name.to_string(),
        None => Input::new()
            .with_prompt("What name do you want to give the stat?")
            .interact_text()
            .unwrap(),
    };

    if repo_config
        .stats
        .as_ref()
        .map_or(false, |stats| stats.contains_key(&stat_name))
    {
        println!(
            "The stat '{}' already exists for the repo '{}'. Please choose a different name.",
            stat_name, repo_name
        );
        return;
    }

    let stat_type =
        Select::with_theme(&my_theme)
            .with_prompt("What type of stat do you want to add?")
            .items(&ALL_MEASUREMENTS)
            .default(0)
            .interact()
            .unwrap();

    let description = Input::new()
        .with_prompt("Give your stat a description")
        .interact_text()
        .unwrap();

    // let stat_params = prompt_build_stat(&ALL_MEASUREMENTS[stat_type]);

    let stat_config =
        UserStatConfig {
            name: stat_name.clone(),
            description,
            type_: ALL_MEASUREMENTS[stat_type].into(),
            path_in_repo: None,
            start: None,
            params: None,
            end: None,
            granularity: None,
        };

    config::add_stat(&repo_name, &stat_name, stat_config);

    println!("Stat '{}' added to repo '{}'", stat_name, repo_name);

    if dialoguer::Confirm::new()
        .with_prompt("Do you want to run this stat now?")
        .interact()
        .unwrap()
    {
        println!("Not implemented yet");
        // Implement run command here
        // println!("Running stat '{}' for repo '{}'", stat_name, repo_name);
    }
}

/*
fn _prompt_build_stat(stat_type: &str) -> std::collections::HashMap<String, String> {
    let mut params = std::collections::HashMap::new();
    //Ask the user questions for each parameter in the stat

    params
}

*/
