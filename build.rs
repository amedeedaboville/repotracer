use std::process::Command;

fn main() {
    // Get the short git hash
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .unwrap();
    let git_hash = String::from_utf8(output.stdout).unwrap();
    // Set the GIT_HASH environment variable for the main compilation
    println!("cargo:rustc-env=GIT_HASH={}", git_hash.trim());

    // Tell cargo to rerun the build script if the git HEAD changes
    println!("cargo:rerun-if-changed=.git/HEAD");
    // Also rerun if the ref HEAD points to changes
    if let Ok(output) = Command::new("git").args(["symbolic-ref", "HEAD"]).output() {
        if let Ok(ref_path) = String::from_utf8(output.stdout) {
            println!("cargo:rerun-if-changed=.git/{}", ref_path.trim());
        }
    }
}
