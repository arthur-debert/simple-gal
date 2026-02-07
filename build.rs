fn main() {
    // Re-run if git HEAD changes (new commits, checkouts, etc.)
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/");

    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let on_tag = std::process::Command::new("git")
        .args(["describe", "--exact-match", "--tags", "HEAD"])
        .output()
        .ok()
        .is_some_and(|o| o.status.success());

    println!("cargo:rustc-env=GIT_HASH={hash}");
    println!("cargo:rustc-env=ON_RELEASE_TAG={on_tag}");
}
