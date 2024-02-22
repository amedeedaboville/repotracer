use indicatif::{ProgressBar, ProgressStyle};

pub fn pb_style() -> ProgressStyle {
    let style = ProgressStyle::default_bar()
        .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {per_sec}")
        .expect("error with progress bar style");
    style
}
pub fn pb_default(total: usize) -> ProgressBar {
    let pb = ProgressBar::new(total as u64);
    pb.set_style(pb_style());
    pb
}
