//! Browser PWA tests — verifies Manifest and Service Worker presence.
//!
//! Run with: `cargo test --test browser_pwa -- --ignored`

use headless_chrome::{Browser, LaunchOptions, Tab};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, OnceLock};

// ---------------------------------------------------------------------------
// Setup helpers (Copied/Adapted from browser_layout.rs)
// ---------------------------------------------------------------------------

fn generated_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/generated")
}

fn ensure_fixtures_built() {
    static BUILT: OnceLock<()> = OnceLock::new();
    BUILT.get_or_init(|| {
        let bin = env!("CARGO_BIN_EXE_simple-gal");
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

        // Ensure the build output directory exists and is clean-ish
        let output_dir = root.join("tests/browser/generated");
        if output_dir.exists() {
            std::fs::remove_dir_all(&output_dir).expect("failed to clean output dir");
        }

        let status = Command::new(bin)
            .args([
                "build",
                "--source",
                root.join("fixtures/browser-content").to_str().unwrap(),
                "--output",
                output_dir.to_str().unwrap(),
                "--temp-dir",
                root.join(".simple-gal-browser-pwa-temp").to_str().unwrap(),
            ])
            .status()
            .expect("failed to run simple-gal");
        assert!(status.success(), "fixture generation failed");
    });
}

fn browser() -> &'static Browser {
    static B: OnceLock<Browser> = OnceLock::new();
    B.get_or_init(|| {
        Browser::new(LaunchOptions {
            window_size: Some((1280, 800)),
            ..Default::default()
        })
        .expect("failed to launch Chrome")
    })
}

fn load_index() -> Arc<Tab> {
    ensure_fixtures_built();
    let tab = browser().new_tab().unwrap();
    let file = generated_dir().join("index.html");
    assert!(file.exists(), "missing: {}", file.display());

    tab.navigate_to(&format!("file://{}", file.display()))
        .unwrap()
        .wait_until_navigated()
        .unwrap();
    tab
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn manifest_link_present() {
    let tab = load_index();
    let val = tab
        .evaluate(
            r#"document.querySelector('link[rel="manifest"]').href"#,
            false,
        )
        .expect("failed to evaluate JS")
        .value
        .expect("no value returned");

    let href = val.as_str().expect("href is not a string");
    assert!(href.ends_with("site.webmanifest"), "href was {}", href);
}

#[test]
#[ignore]
fn apple_touch_icon_present() {
    let tab = load_index();
    let val = tab
        .evaluate(
            r#"document.querySelector('link[rel="apple-touch-icon"]').href"#,
            false,
        )
        .expect("failed to evaluate JS")
        .value
        .expect("no value returned");

    let href = val.as_str().expect("href is not a string");
    assert!(href.ends_with("apple-touch-icon.png"), "href was {}", href);
}

#[test]
#[ignore]
fn service_worker_registration_present() {
    let tab = load_index();
    // We can't easily test if SW *runs* on file://, but we can check the script tag exists
    // and contains the registration code.
    let val = tab
        .evaluate(
            r#"(function() {
                const scripts = Array.from(document.querySelectorAll('script'));
                return scripts.some(s => s.textContent.includes('navigator.serviceWorker.register'));
            })()"#, 
            false
        )
        .expect("failed to evaluate JS")
        .value
        .expect("no value returned");

    assert!(
        val.as_bool().unwrap_or(false),
        "Service Worker registration script not found"
    );
}

#[test]
#[ignore]
fn static_files_copied() {
    ensure_fixtures_built();
    let dir = generated_dir();
    assert!(
        dir.join("site.webmanifest").exists(),
        "site.webmanifest missing"
    );
    assert!(dir.join("sw.js").exists(), "sw.js missing");
    assert!(dir.join("icon-192.png").exists(), "icon-192.png missing");
    assert!(dir.join("icon-512.png").exists(), "icon-512.png missing");
    assert!(dir.join("favicon.png").exists(), "favicon.png missing");
}
