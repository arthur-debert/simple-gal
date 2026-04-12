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

// ============================================================================
// --format ndjson
// ============================================================================

/// Parse NDJSON output: each line is a self-contained JSON object.
fn parse_ndjson_lines(bytes: &[u8]) -> Vec<serde_json::Value> {
    let s = String::from_utf8_lossy(bytes);
    s.lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("bad JSON line: {e}\n---\n{l}")))
        .collect()
}

#[test]
fn ndjson_check_emits_single_result_line() {
    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--format",
            "ndjson",
            "check",
        ])
        .output()
        .expect("run simple-gal");
    assert!(output.status.success());
    let lines = parse_ndjson_lines(&output.stdout);
    assert_eq!(lines.len(), 1, "check should emit exactly 1 line");
    assert_eq!(lines[0]["type"], "result");
    assert_eq!(lines[0]["ok"], true);
    assert_eq!(lines[0]["command"], "check");
}

#[test]
fn ndjson_scan_emits_single_result_line() {
    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--format",
            "ndjson",
            "scan",
        ])
        .output()
        .expect("run simple-gal");
    assert!(output.status.success());
    let lines = parse_ndjson_lines(&output.stdout);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["type"], "result");
    assert_eq!(lines[0]["command"], "scan");
    assert!(lines[0]["data"]["counts"]["albums"].as_u64().unwrap() > 0);
}

#[test]
fn ndjson_build_streams_progress_then_result() {
    let tmp = TempDir::new().unwrap();
    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--output",
            tmp.path().join("dist").to_str().unwrap(),
            "--temp-dir",
            tmp.path().join("temp").to_str().unwrap(),
            "--format",
            "ndjson",
            "build",
            "--no-cache",
        ])
        .output()
        .expect("run simple-gal");
    assert!(
        output.status.success(),
        "exit={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let lines = parse_ndjson_lines(&output.stdout);
    assert!(
        lines.len() >= 2,
        "build should emit progress + result, got {} lines",
        lines.len()
    );

    // All lines except the last must be progress events.
    for line in &lines[..lines.len() - 1] {
        assert_eq!(
            line["type"], "progress",
            "non-final line should be progress: {line}"
        );
        let event = line["event"].as_str().unwrap();
        assert!(
            event == "album_started" || event == "image_processed" || event == "cache_pruned",
            "unexpected event type: {event}"
        );
    }

    // Last line must be the result envelope.
    let last = lines.last().unwrap();
    assert_eq!(last["type"], "result");
    assert_eq!(last["ok"], true);
    assert_eq!(last["command"], "build");
    assert!(last["data"]["counts"]["albums"].as_u64().unwrap() > 0);
}

#[test]
fn ndjson_error_is_compact_single_line() {
    let tmp = TempDir::new().unwrap();
    let missing = missing_source(&tmp);
    let output = simple_gal()
        .args([
            "--source",
            missing.to_str().unwrap(),
            "--format",
            "ndjson",
            "scan",
        ])
        .output()
        .expect("run simple-gal");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());

    // stderr should be exactly one line of compact JSON.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr_lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(
        stderr_lines.len(),
        1,
        "ndjson error should be 1 line, got: {stderr}"
    );
    let v: serde_json::Value = serde_json::from_str(stderr_lines[0])
        .unwrap_or_else(|e| panic!("bad JSON: {e}\n{}", stderr_lines[0]));
    assert_eq!(v["ok"], false);
    assert_eq!(v["kind"], "scan");
}

#[test]
fn ndjson_process_streams_progress_then_result() {
    let tmp = TempDir::new().unwrap();
    let temp_dir = tmp.path().join("temp");

    // Run scan first so that the manifest exists for the process stage.
    let scan_output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--temp-dir",
            temp_dir.to_str().unwrap(),
            "scan",
            "--save-manifest",
        ])
        .output()
        .expect("run simple-gal scan");
    assert!(
        scan_output.status.success(),
        "scan failed: {}",
        String::from_utf8_lossy(&scan_output.stderr)
    );

    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--temp-dir",
            temp_dir.to_str().unwrap(),
            "--format",
            "ndjson",
            "process",
            "--no-cache",
        ])
        .output()
        .expect("run simple-gal process");
    assert!(
        output.status.success(),
        "exit={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let lines = parse_ndjson_lines(&output.stdout);
    assert!(
        lines.len() >= 2,
        "process should emit progress + result, got {} lines",
        lines.len()
    );

    // All lines except the last must be progress events.
    for line in &lines[..lines.len() - 1] {
        assert_eq!(
            line["type"], "progress",
            "non-final line should be progress: {line}"
        );
        let event = line["event"].as_str().unwrap();
        assert!(
            event == "album_started" || event == "image_processed" || event == "cache_pruned",
            "unexpected event type: {event}"
        );
    }

    // Last line must be the result envelope.
    let last = lines.last().unwrap();
    assert_eq!(last["type"], "result");
    assert_eq!(last["ok"], true);
    assert_eq!(last["command"], "process");
    assert!(last["data"]["cache"]["total"].as_u64().unwrap() > 0);
}

#[test]
fn ndjson_each_line_is_compact_no_pretty_print() {
    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--format",
            "ndjson",
            "check",
        ])
        .output()
        .expect("run simple-gal");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Compact JSON has no internal newlines — one JSON object = one line.
    let non_empty: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(non_empty.len(), 1);
    // No leading whitespace (pretty-print indentation).
    assert!(
        !non_empty[0].starts_with(' '),
        "ndjson line should be compact: {}",
        non_empty[0]
    );
}

// ============================================================================
// --format progress
// ============================================================================

#[test]
fn progress_check_emits_single_result_line() {
    // Non-build commands should fall back to ndjson-style single result.
    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--format",
            "progress",
            "check",
        ])
        .output()
        .expect("run simple-gal");
    assert!(output.status.success());
    let lines = parse_ndjson_lines(&output.stdout);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["type"], "result");
    assert_eq!(lines[0]["command"], "check");
}

#[test]
fn progress_build_streams_percent_and_counters() {
    let tmp = TempDir::new().unwrap();
    let output = simple_gal()
        .args([
            "--source",
            fixtures_dir().to_str().unwrap(),
            "--output",
            tmp.path().join("dist").to_str().unwrap(),
            "--temp-dir",
            tmp.path().join("temp").to_str().unwrap(),
            "--format",
            "progress",
            "build",
            "--no-cache",
        ])
        .output()
        .expect("run simple-gal");
    assert!(
        output.status.success(),
        "exit={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );

    let lines = parse_ndjson_lines(&output.stdout);
    // At minimum: scan progress + N image progress + generate progress + result
    assert!(
        lines.len() >= 4,
        "progress build should emit >=4 lines, got {}",
        lines.len()
    );

    // First line: scan complete at 2%.
    assert_eq!(lines[0]["type"], "progress");
    assert_eq!(lines[0]["stage"], "scan");
    assert_eq!(lines[0]["percent"], 2.0);
    assert!(lines[0]["images_total"].as_u64().unwrap() > 0);
    assert!(lines[0]["variants_total"].as_u64().unwrap() > 0);

    // Middle lines: process stage with increasing images_done.
    let process_lines: Vec<&serde_json::Value> =
        lines.iter().filter(|l| l["stage"] == "process").collect();
    assert!(!process_lines.is_empty(), "should have process events");

    // images_done should increase monotonically.
    let images_done: Vec<u64> = process_lines
        .iter()
        .map(|l| l["images_done"].as_u64().unwrap())
        .collect();
    for w in images_done.windows(2) {
        assert!(
            w[1] >= w[0],
            "images_done should be monotonic: {:?}",
            images_done
        );
    }

    // Percent should be > 2 and < 100 for all process events.
    for line in &process_lines {
        let pct = line["percent"].as_f64().unwrap();
        assert!(pct > 2.0, "process percent should be > 2: {pct}");
        assert!(pct <= 92.0, "process percent should be <= 92: {pct}");
    }

    // Second-to-last progress line: generate stage.
    let last_progress = lines
        .iter()
        .rev()
        .find(|l| l["type"] == "progress")
        .unwrap();
    assert_eq!(last_progress["stage"], "generate");
    assert_eq!(last_progress["percent"], 92.0);

    // Last line: result.
    let last = lines.last().unwrap();
    assert_eq!(last["type"], "result");
    assert_eq!(last["command"], "build");
}

#[test]
fn progress_error_is_compact_single_line() {
    let tmp = TempDir::new().unwrap();
    let missing = missing_source(&tmp);
    let output = simple_gal()
        .args([
            "--source",
            missing.to_str().unwrap(),
            "--format",
            "progress",
            "scan",
        ])
        .output()
        .expect("run simple-gal");

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr_lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(stderr_lines.len(), 1, "error should be 1 line: {stderr}");
    let v: serde_json::Value = serde_json::from_str(stderr_lines[0]).unwrap();
    assert_eq!(v["ok"], false);
}
