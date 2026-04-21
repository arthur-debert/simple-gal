//! End-to-end tests for the `[auto_indexing].sync_source_files` build hook.
//!
//! Each test builds a tiny self-contained content tree in a TempDir, runs
//! `simple-gal build` against it, and asserts on the final state of both
//! the source tree (renamed in place when the hook is enabled; left alone
//! otherwise) and the generated output.
//!
//! Phase 5 of the data-model refactor collapsed the earlier four-mode
//! enum (`off` / `source_only` / `export_only` / `both`) into a single
//! boolean. The `source_only` / `both` distinction was always cosmetic
//! (identical on-disk effect); `export_only` never had an implementation.
//! These tests exercise the one remaining toggle plus a migration check.

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn simple_gal() -> Command {
    Command::new(env!("CARGO_BIN_EXE_simple-gal"))
}

/// Reuse a real JPEG fixture from `fixtures/content/` so the decoder
/// doesn't panic when the build reaches the process stage. The tests
/// only care about the pre-scan rename step; this just needs to be a
/// valid image the pipeline can open.
fn sample_image_bytes() -> Vec<u8> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/content/010-Landscapes/001-dawn.jpg");
    fs::read(path).expect("fixture image missing")
}

/// Build a content tree at `root` with two images numbered at arbitrary
/// sparse positions, a `config.toml` with the given `sync_source_files`
/// value, and a minimal site description.
fn seed_content(root: &Path, sync: bool) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("config.toml"),
        format!(
            r#"
site_title = "Test"

[auto_indexing]
sync_source_files = {sync}
spacing = 1
padding = 3

[images]
sizes = [400]
quality = 70
"#
        ),
    )
    .unwrap();
    let album = root.join("5-Album");
    fs::create_dir_all(&album).unwrap();
    let bytes = sample_image_bytes();
    fs::write(album.join("1-first.jpg"), &bytes).unwrap();
    fs::write(album.join("10-second.jpg"), &bytes).unwrap();
}

fn run_build(source: &Path, temp: &Path, output: &Path) -> std::process::Output {
    simple_gal()
        .args([
            "--source",
            source.to_str().unwrap(),
            "--temp-dir",
            temp.to_str().unwrap(),
            "--output",
            output.to_str().unwrap(),
            "--quiet",
            "build",
        ])
        .output()
        .expect("build command failed to spawn")
}

/// `run_build` wrapper that asserts the build command succeeded. The
/// fixture image is a real JPEG, so the pipeline goes end-to-end.
fn run_build_ok(source: &Path, temp: &Path, output: &Path) {
    let result = run_build(source, temp, output);
    assert!(
        result.status.success(),
        "build failed unexpectedly.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

// ----------------------------------------------------------------------------
// sync_source_files = false (default)
// ----------------------------------------------------------------------------

#[test]
fn sync_off_leaves_source_alone() {
    let workspace = TempDir::new().unwrap();
    let source = workspace.path().join("content");
    let temp = workspace.path().join("temp");
    let output = workspace.path().join("dist");
    seed_content(&source, false);

    run_build_ok(&source, &temp, &output);

    // Source filenames unchanged.
    assert!(source.join("5-Album/1-first.jpg").exists());
    assert!(source.join("5-Album/10-second.jpg").exists());
    assert!(!source.join("010-Album").exists());
}

// ----------------------------------------------------------------------------
// sync_source_files = true
// ----------------------------------------------------------------------------

#[test]
fn sync_on_renames_source_in_place() {
    let workspace = TempDir::new().unwrap();
    let source = workspace.path().join("content");
    let temp = workspace.path().join("temp");
    let output = workspace.path().join("dist");
    seed_content(&source, true);

    run_build_ok(&source, &temp, &output);

    // Album dir renumbered at root (position 1 → 010).
    assert!(source.join("010-Album").exists());
    assert!(!source.join("5-Album").exists());
    // Images inside renumbered (positions 1, 2 → 010, 020).
    assert!(source.join("010-Album/010-first.jpg").exists());
    assert!(source.join("010-Album/020-second.jpg").exists());
    assert!(!source.join("010-Album/1-first.jpg").exists());
}

// ----------------------------------------------------------------------------
// Migration: old `auto = "..."` field is loud-rejected (Phase 5)
// ----------------------------------------------------------------------------

#[test]
fn legacy_auto_field_errors_clearly() {
    let workspace = TempDir::new().unwrap();
    let source = workspace.path().join("content");
    let temp = workspace.path().join("temp");
    let output = workspace.path().join("dist");
    fs::create_dir_all(&source).unwrap();
    fs::write(
        source.join("config.toml"),
        r#"
[auto_indexing]
auto = "source_only"
"#,
    )
    .unwrap();
    let album = source.join("5-Album");
    fs::create_dir_all(&album).unwrap();
    fs::write(album.join("1-first.jpg"), sample_image_bytes()).unwrap();

    let result = run_build(&source, &temp, &output);
    assert!(
        !result.status.success(),
        "expected non-zero exit on legacy field"
    );
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("unknown field") && stderr.contains("`auto`"),
        "expected stderr to flag the unknown `auto` field specifically, got: {stderr}"
    );
    assert!(
        stderr.contains("sync_source_files"),
        "expected stderr to point users at the new `sync_source_files` field, got: {stderr}"
    );
    // Source untouched because config load fails before the hook runs.
    assert!(source.join("5-Album/1-first.jpg").exists());
}

// ----------------------------------------------------------------------------
// Cache invalidation
// ----------------------------------------------------------------------------

#[test]
fn sync_on_invalidates_processed_dir_on_rename() {
    // Seed a stale `processed/` directory that would otherwise be reused,
    // confirm the hook wipes it when it actually renames something.
    let workspace = TempDir::new().unwrap();
    let source = workspace.path().join("content");
    let temp = workspace.path().join("temp");
    let output = workspace.path().join("dist");
    seed_content(&source, true);

    let processed = temp.join("processed");
    fs::create_dir_all(&processed).unwrap();
    let stale_marker = processed.join("stale-marker.txt");
    fs::write(&stale_marker, "from a previous build").unwrap();

    run_build_ok(&source, &temp, &output);

    assert!(
        !stale_marker.exists(),
        "expected processed/ to have been wiped after rename"
    );
}

#[test]
fn sync_on_preserves_processed_dir_when_no_renames_needed() {
    // If the source is already normalized, the hook shouldn't touch the
    // cache directory.
    let workspace = TempDir::new().unwrap();
    let source = workspace.path().join("content");
    let temp = workspace.path().join("temp");
    let output = workspace.path().join("dist");
    seed_content(&source, true);

    // First build: renames happen, cache gets populated by process stage.
    run_build_ok(&source, &temp, &output);

    // Plant a marker in the freshly-built processed/ dir.
    let processed = temp.join("processed");
    assert!(
        processed.exists(),
        "expected process stage to create {}",
        processed.display()
    );
    let marker = processed.join("second-build-marker.txt");
    fs::write(&marker, "should survive").unwrap();

    // Second build on already-normalized tree: no renames, no wipe.
    run_build_ok(&source, &temp, &output);

    assert!(
        marker.exists(),
        "expected cache marker to survive a no-op auto-reindex"
    );
}
