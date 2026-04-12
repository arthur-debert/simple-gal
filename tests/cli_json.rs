//! Integration tests for the `--format json` envelope contract.
//!
//! Every command emits a tagged `OkEnvelope` on success (`{ok: true, command,
//! data}`) and an `ErrorEnvelope` on failure (`{ok: false, kind, message,
//! causes, config?}`). Exit codes map to the `ErrorKind`. These tests pin that
//! contract so automation clients can rely on it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

fn simple_gal() -> Command {
    Command::new(env!("CARGO_BIN_EXE_simple-gal"))
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/content")
}

fn parse_json(bytes: &[u8]) -> serde_json::Value {
    let s = String::from_utf8_lossy(bytes);
    serde_json::from_str(&s).unwrap_or_else(|e| panic!("not valid JSON: {e}\n---\n{s}\n---"))
}

// ============================================================================
// Success envelopes
// ============================================================================

#[test]
fn check_json_envelope() {
    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--format",
            "json",
            "check",
        ])
        .output()
        .expect("run simple-gal");
    assert!(output.status.success(), "exit={}", output.status);
    let v = parse_json(&output.stdout);
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "check");
    assert_eq!(v["data"]["valid"], true);
    assert!(v["data"]["counts"]["albums"].as_u64().unwrap() > 0);
}

#[test]
fn config_gen_json_envelope() {
    let output = simple_gal()
        .args(["--format", "json", "config", "gen"])
        .output()
        .expect("run simple-gal");
    assert!(
        output.status.success(),
        "exit={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let v = parse_json(&output.stdout);
    assert_eq!(v["command"], "config");
    assert_eq!(v["data"]["action"], "gen");
    let toml = v["data"]["toml"].as_str().unwrap();
    assert!(toml.contains("site_title"));
}

#[test]
fn config_schema_json_envelope() {
    let output = simple_gal()
        .args(["--format", "json", "config", "schema"])
        .output()
        .expect("run simple-gal");
    assert!(
        output.status.success(),
        "exit={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let v = parse_json(&output.stdout);
    assert_eq!(v["command"], "config");
    assert_eq!(v["data"]["action"], "schema");
    let schema = &v["data"]["schema"];
    assert_eq!(
        schema["$schema"], "https://json-schema.org/draft/2020-12/schema",
        "schema should declare draft 2020-12 dialect"
    );
    assert_eq!(schema["type"], "object");
    // The top-level SiteConfig must be present in the schema's properties.
    assert!(schema["properties"]["site_title"].is_object());
    assert!(schema["properties"]["colors"]["properties"]["light"].is_object());
}

#[test]
fn scan_envelope_has_counts_and_source() {
    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--format",
            "json",
            "scan",
        ])
        .output()
        .expect("run simple-gal");
    assert!(output.status.success());
    let v = parse_json(&output.stdout);
    assert_eq!(v["command"], "scan");
    assert!(v["data"]["counts"]["albums"].as_u64().unwrap() > 0);
    assert!(v["data"]["counts"]["images"].as_u64().unwrap() > 0);
    assert!(v["data"]["manifest"]["navigation"].is_array());
}

// ============================================================================
// Error envelopes + exit codes
// ============================================================================

#[test]
fn config_error_json_envelope() {
    // Unquoted CSS value — the same kind of failure clapfig renders in text
    // mode. In JSON mode it becomes an ErrorEnvelope on stderr with kind
    // "config", a snippet, line, column, and exit code 3.
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("config.toml"),
        "site_title = \"Bad\"\n\n[theme]\nthumbnail_gap = 0.1rem\n",
    )
    .unwrap();

    let output = simple_gal()
        .args([
            "--source",
            tmp.path().to_str().unwrap(),
            "--format",
            "json",
            "scan",
        ])
        .output()
        .expect("run simple-gal");

    assert!(!output.status.success(), "should fail on bad config");
    assert_eq!(output.status.code(), Some(3), "config error exit code");

    // stdout must be empty: on failure the envelope goes to stderr.
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty on error, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    let v = parse_json(&output.stderr);
    assert_eq!(v["ok"], false);
    assert_eq!(v["kind"], "config");
    assert!(!v["message"].as_str().unwrap().is_empty());
    let cfg = &v["config"];
    assert!(cfg["path"].as_str().unwrap().ends_with("config.toml"));
    assert_eq!(cfg["line"], 4);
    assert!(cfg["snippet"].as_str().unwrap().contains("thumbnail_gap"));
}

/// Build a guaranteed-nonexistent path inside a `TempDir` (portable
/// across Unix/Windows, and doesn't collide with anything real).
fn missing_source(tmp: &TempDir) -> PathBuf {
    tmp.path().join("does-not-exist-xyz")
}

#[test]
fn scan_error_json_envelope_missing_source() {
    // Nonexistent source directory — scan stage fails with an IO error.
    // stage classification wins: kind="scan", exit code = 5.
    let tmp = TempDir::new().unwrap();
    let missing = missing_source(&tmp);
    let output = simple_gal()
        .args([
            "--source",
            missing.to_str().unwrap(),
            "--format",
            "json",
            "scan",
        ])
        .output()
        .expect("run simple-gal");

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(5));
    assert!(output.stdout.is_empty());
    let v = parse_json(&output.stderr);
    assert_eq!(v["ok"], false);
    assert_eq!(v["kind"], "scan");
}

#[test]
fn text_mode_error_does_not_emit_json() {
    // Regression: in text mode the error path stays human-readable; no
    // stray JSON on stderr.
    let tmp = TempDir::new().unwrap();
    let missing = missing_source(&tmp);
    let output = simple_gal()
        .args([
            "--source",
            missing.to_str().unwrap(),
            "--format",
            "text",
            "scan",
        ])
        .output()
        .expect("run simple-gal");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("Error:"));
    assert!(
        serde_json::from_str::<serde_json::Value>(&stderr).is_err(),
        "text-mode stderr must not be JSON: {stderr}"
    );
}

// ============================================================================
// --quiet
// ============================================================================

#[test]
fn quiet_suppresses_text_output() {
    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--quiet",
            "check",
        ])
        .output()
        .expect("run simple-gal");

    assert!(output.status.success());
    assert!(
        output.stdout.is_empty(),
        "--quiet should suppress stdout in text mode, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn quiet_suppresses_scan_text_tree() {
    // `scan` has a text-mode renderer too; --quiet must suppress it
    // just like every other command.
    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--format",
            "text",
            "--quiet",
            "scan",
        ])
        .output()
        .expect("run simple-gal");

    assert!(output.status.success());
    assert!(
        output.stdout.is_empty(),
        "--quiet should suppress scan's text tree, got: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}
