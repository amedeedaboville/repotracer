use std::env;
use std::process::Command;

fn get_git_hash() -> String {
    if Command::new("git").arg("--version").output().is_ok() {
        if let Ok(output) = Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
        {
            if let Ok(hash) = String::from_utf8(output.stdout) {
                if !hash.trim().is_empty() {
                    return hash.trim().to_string();
                }
            }
        }
    }

    if let Ok(github_sha) = env::var("GITHUB_SHA") {
        return github_sha[..7].to_string();
    }

    "local".to_string()
}

fn main() {
    let git_hash = get_git_hash();
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);

    if Command::new("git").arg("--version").output().is_ok() {
        println!("cargo:rerun-if-changed=.git/HEAD");
        if let Ok(output) = Command::new("git").args(["symbolic-ref", "HEAD"]).output() {
            if let Ok(ref_path) = String::from_utf8(output.stdout) {
                println!("cargo:rerun-if-changed=.git/{}", ref_path.trim());
            }
        }
    }
}
