use indicatif::ProgressStyle;

pub fn pb_style() -> ProgressStyle {
    let style = ProgressStyle::default_bar()
        .template("[{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>7}/{len:7} ({eta}) {per_sec}")
        .expect("error with progress bar style");
    style
}
