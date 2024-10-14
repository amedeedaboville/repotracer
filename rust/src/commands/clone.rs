use indicatif::{ProgressBar, ProgressStyle};

use crate::repo;

pub fn clone_command(clone_urls: Vec<String>) {
    let progress_bar = ProgressBar::new(clone_urls.len() as u64);
    progress_bar.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {msg}",
            )
            .unwrap(),
    );

    for clone_url in clone_urls.iter() {
        progress_bar.set_message(clone_url.to_string());
        match repo::clone_repo(clone_url) {
            Ok(_) => {}
            Err(e) => {
                progress_bar.println(format!("Failed to clone {}: {}", clone_url, e));
            }
        }
        progress_bar.inc(1); // Increment the progress bar after each attempt
                             //Sleep 1s in between repos
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    progress_bar.finish_with_message("Cloning completed");
}
