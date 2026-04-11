//! End-to-end check that a bad `config.toml` produces a clapfig-rendered
//! error (source snippet + caret) instead of the old Debug dump.

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn simple_gal() -> Command {
    Command::new(env!("CARGO_BIN_EXE_simple-gal"))
}

fn write_bad_config(dir: &Path) {
    // Unquoted CSS value — the exact class of mistake the user hit in
    // practice. TOML parses `0.1` as a float and then chokes on `rem`.
    fs::write(
        dir.join("config.toml"),
        "site_title = \"Bad\"\n\n[theme]\nthumbnail_gap = 0.1rem\n",
    )
    .unwrap();
}

#[test]
fn build_with_bad_config_renders_clapfig_plain_error() {
    let tmp = TempDir::new().unwrap();
    write_bad_config(tmp.path());

    let output = simple_gal()
        .args(["--source", tmp.path().to_str().unwrap(), "build"])
        .output()
        .expect("failed to run simple-gal");

    assert!(
        !output.status.success(),
        "expected non-zero exit on bad config, got: {}",
        output.status
    );

    let stderr = String::from_utf8_lossy(&output.stderr);

    // clapfig's plain renderer produces a multi-line message with the
    // "error: failed to parse config file" header, file path, a source
    // snippet, and caret markers. Make sure we're seeing that — not the
    // bare Debug dump the CLI used to show.
    assert!(
        stderr.contains("failed to parse config file"),
        "missing parse error header. stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("config.toml"),
        "missing file path. stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("thumbnail_gap"),
        "missing source snippet. stderr:\n{stderr}"
    );
    assert!(
        stderr.contains('^'),
        "missing caret marker. stderr:\n{stderr}"
    );

    // Regression: the old output exposed internal enum structure.
    assert!(
        !stderr.contains("Config(Toml("),
        "stderr should not contain raw Debug output. stderr:\n{stderr}"
    );
}

#[test]
fn scan_with_bad_config_renders_clapfig_plain_error() {
    // Same check for the scan command, which also touches config loading.
    let tmp = TempDir::new().unwrap();
    write_bad_config(tmp.path());

    let output = simple_gal()
        .args(["--source", tmp.path().to_str().unwrap(), "scan"])
        .output()
        .expect("failed to run simple-gal");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("failed to parse config file"));
    assert!(stderr.contains("config.toml"));
    assert!(stderr.contains('^'));
    assert!(!stderr.contains("Config(Toml("));
}
