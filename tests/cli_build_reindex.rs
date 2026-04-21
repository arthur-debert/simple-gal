//! End-to-end tests for the `[auto_indexing].auto` build hook.
//!
//! Each test builds a tiny self-contained content tree in a TempDir, runs
//! `simple-gal build` against it, and asserts on the final state of both
//! the source tree (`source_only` and `both` rename in place; `off` does
//! not) and the generated output.
//!
//! Focus is on the hook's on-disk effects before scan; the process stage
//! may fail on the minimal reused fixture image but the rename work we
//! assert on has already happened by then.
//!
//! `export_only` is covered only for its current unsupported behavior —
//! we verify it errors with a clear "not yet supported" message rather
//! than exercising a successful rename/build path.

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
/// sparse positions, a `config.toml` enabling the given auto mode, and a
/// minimal site description.
fn seed_content(root: &Path, auto_mode: &str) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("config.toml"),
        format!(
            r#"
site_title = "Test"

[auto_indexing]
auto = "{auto_mode}"
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
/// minimal placeholder image used here is a real JPEG, so the pipeline
/// goes end-to-end. Using this on passing-path tests prevents a silent
/// `process` / `generate` failure from faking a green test via surviving
/// filesystem state.
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
// off (default behavior)
// ----------------------------------------------------------------------------

#[test]
fn auto_off_leaves_source_alone() {
    let workspace = TempDir::new().unwrap();
    let source = workspace.path().join("content");
    let temp = workspace.path().join("temp");
    let output = workspace.path().join("dist");
    seed_content(&source, "off");

    run_build_ok(&source, &temp, &output);

    // Source filenames unchanged.
    assert!(source.join("5-Album/1-first.jpg").exists());
    assert!(source.join("5-Album/10-second.jpg").exists());
    assert!(!source.join("010-Album").exists());
}

// ----------------------------------------------------------------------------
// source_only
// ----------------------------------------------------------------------------

#[test]
fn auto_source_only_renames_source_in_place() {
    let workspace = TempDir::new().unwrap();
    let source = workspace.path().join("content");
    let temp = workspace.path().join("temp");
    let output = workspace.path().join("dist");
    seed_content(&source, "source_only");

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
// both (same on-disk effect as source_only)
// ----------------------------------------------------------------------------

#[test]
fn auto_both_renames_source_like_source_only() {
    let workspace = TempDir::new().unwrap();
    let source = workspace.path().join("content");
    let temp = workspace.path().join("temp");
    let output = workspace.path().join("dist");
    seed_content(&source, "both");

    run_build_ok(&source, &temp, &output);

    assert!(source.join("010-Album/010-first.jpg").exists());
    assert!(source.join("010-Album/020-second.jpg").exists());
}

// ----------------------------------------------------------------------------
// export_only (explicit not-yet-supported error)
// ----------------------------------------------------------------------------

#[test]
fn auto_export_only_errors_clearly() {
    let workspace = TempDir::new().unwrap();
    let source = workspace.path().join("content");
    let temp = workspace.path().join("temp");
    let output = workspace.path().join("dist");
    seed_content(&source, "export_only");

    let result = run_build(&source, &temp, &output);
    assert!(!result.status.success(), "expected non-zero exit");
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("export_only") && stderr.contains("not yet supported"),
        "stderr did not mention export_only unsupported: {stderr}"
    );
    // Source untouched.
    assert!(source.join("5-Album/1-first.jpg").exists());
}

// ----------------------------------------------------------------------------
// Cache invalidation
// ----------------------------------------------------------------------------

#[test]
fn source_only_invalidates_processed_dir_on_rename() {
    // Seed a stale `processed/` directory that would otherwise be reused,
    // confirm the hook wipes it when it actually renames something.
    let workspace = TempDir::new().unwrap();
    let source = workspace.path().join("content");
    let temp = workspace.path().join("temp");
    let output = workspace.path().join("dist");
    seed_content(&source, "source_only");

    let processed = temp.join("processed");
    fs::create_dir_all(&processed).unwrap();
    let stale_marker = processed.join("stale-marker.txt");
    fs::write(&stale_marker, "from a previous build").unwrap();

    run_build_ok(&source, &temp, &output);

    // The stale marker should be gone because the hook wiped the cache.
    assert!(
        !stale_marker.exists(),
        "expected processed/ to have been wiped after rename"
    );
}

#[test]
fn source_only_preserves_processed_dir_when_no_renames_needed() {
    // If the source is already normalized, the hook shouldn't touch the
    // cache directory.
    let workspace = TempDir::new().unwrap();
    let source = workspace.path().join("content");
    let temp = workspace.path().join("temp");
    let output = workspace.path().join("dist");
    seed_content(&source, "source_only");

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
