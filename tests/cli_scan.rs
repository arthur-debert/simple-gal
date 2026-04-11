//! CLI integration tests for the `scan` subcommand.
//!
//! Tests `--format` (json/text) and `--save-manifest` flags.

use std::path::Path;
use std::process::Command;

fn simple_gal() -> Command {
    Command::new(env!("CARGO_BIN_EXE_simple-gal"))
}

fn fixtures_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/content")
        .leak()
}

// =========================================================================
// --format (default = json)
// =========================================================================

#[test]
fn scan_default_format_is_json() {
    let output = simple_gal()
        .args(["--source", fixtures_dir().to_str().unwrap(), "scan"])
        .output()
        .expect("failed to run simple-gal");

    assert!(output.status.success(), "exit code: {}", output.status);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("default output should be valid JSON");
    // New envelope format: ok/command/data wrapper around the manifest.
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["command"], "scan");
    let data = &parsed["data"];
    assert!(data.get("manifest").is_some());
    assert!(data["manifest"].get("navigation").is_some());
    assert!(data["manifest"].get("albums").is_some());
    assert!(data.get("counts").is_some());
}

#[test]
fn scan_format_json_outputs_valid_manifest() {
    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--format",
            "json",
            "scan",
        ])
        .output()
        .expect("failed to run simple-gal");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let albums = parsed["data"]["manifest"]["albums"].as_array().unwrap();
    assert!(!albums.is_empty(), "should discover at least one album");
}

#[test]
fn scan_format_text_outputs_tree() {
    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--format",
            "text",
            "scan",
        ])
        .output()
        .expect("failed to run simple-gal");

    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Text output starts with "Albums" and is NOT valid JSON
    assert!(
        stdout.contains("Albums"),
        "text output should start with Albums section"
    );
    assert!(
        serde_json::from_str::<serde_json::Value>(&stdout).is_err(),
        "text output should not be valid JSON"
    );
}

// =========================================================================
// --save-manifest
// =========================================================================

#[test]
fn scan_does_not_save_manifest_by_default() {
    let tmp = tempfile::TempDir::new().unwrap();
    let manifest_path = tmp.path().join("manifest.json");

    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--temp-dir",
            tmp.path().to_str().unwrap(),
            "scan",
        ])
        .output()
        .expect("failed to run simple-gal");

    assert!(output.status.success());
    assert!(
        !manifest_path.exists(),
        "manifest.json should NOT be created without --save-manifest"
    );
}

#[test]
fn scan_save_manifest_default_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let manifest_path = tmp.path().join("manifest.json");

    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--temp-dir",
            tmp.path().to_str().unwrap(),
            "scan",
            "--save-manifest",
        ])
        .output()
        .expect("failed to run simple-gal");

    assert!(output.status.success());
    assert!(
        manifest_path.exists(),
        "manifest.json should be created at temp-dir with --save-manifest (no value)"
    );

    let content = std::fs::read_to_string(&manifest_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(parsed.get("albums").is_some());
}

#[test]
fn scan_save_manifest_custom_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let custom_path = tmp.path().join("custom/output/my-manifest.json");

    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "scan",
            "--save-manifest",
            custom_path.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run simple-gal");

    assert!(output.status.success());
    assert!(
        custom_path.exists(),
        "manifest should be saved to the custom path"
    );

    let content = std::fs::read_to_string(&custom_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(parsed.get("albums").is_some());
}

// =========================================================================
// --format + --save-manifest combined
// =========================================================================

#[test]
fn scan_text_format_with_save_manifest() {
    let tmp = tempfile::TempDir::new().unwrap();
    let manifest_path = tmp.path().join("manifest.json");

    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--temp-dir",
            tmp.path().to_str().unwrap(),
            "--format",
            "text",
            "scan",
            "--save-manifest",
        ])
        .output()
        .expect("failed to run simple-gal");

    assert!(output.status.success());

    // stdout is text
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Albums"));

    // but the file is JSON — saved_manifest is always the raw scan manifest
    assert!(manifest_path.exists());
    let content = std::fs::read_to_string(&manifest_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(parsed.get("albums").is_some());
}

#[test]
fn scan_json_format_with_save_manifest() {
    let tmp = tempfile::TempDir::new().unwrap();
    let manifest_path = tmp.path().join("manifest.json");

    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--temp-dir",
            tmp.path().to_str().unwrap(),
            "--format",
            "json",
            "scan",
            "--save-manifest",
        ])
        .output()
        .expect("failed to run simple-gal");

    assert!(output.status.success());

    // stdout is the envelope
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("stdout should be valid JSON");
    assert_eq!(parsed["ok"], true);
    assert_eq!(
        parsed["data"]["saved_manifest_path"],
        manifest_path.to_string_lossy().as_ref()
    );

    // file is the raw manifest (albums at top level)
    let content = std::fs::read_to_string(&manifest_path).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(saved.get("albums").is_some());
}
