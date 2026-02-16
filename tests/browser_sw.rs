//! Service worker integration tests — verifies SW lifecycle and caching strategies.
//!
//! These tests use headless Chrome over a local HTTP server (service workers
//! require HTTP, not file://) to exercise cached and non-cached code paths.
//!
//! Run with: `cargo test --test browser_sw -- --ignored`

use headless_chrome::{Browser, LaunchOptions, Tab};
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

// ===========================================================================
// Minimal HTTP server for SW testing (SWs require HTTP, not file://)
// ===========================================================================

struct TestServer {
    port: u16,
    _stop: std::sync::mpsc::Sender<()>,
}

impl TestServer {
    fn start(root: PathBuf) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = std::sync::mpsc::channel::<()>();

        thread::spawn(move || {
            listener.set_nonblocking(true).unwrap();
            loop {
                if rx.try_recv().is_ok() {
                    break;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let root = root.clone();
                        thread::spawn(move || serve_request(stream, &root));
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Self { port, _stop: tx }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

fn serve_request(mut stream: std::net::TcpStream, root: &std::path::Path) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = [0u8; 4096];
    let n = match stream.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return,
    };
    let request = String::from_utf8_lossy(&buf[..n]);
    let path = request.split_whitespace().nth(1).unwrap_or("/");
    let rel = path.trim_start_matches('/');
    let file_path = if rel.is_empty() {
        root.join("index.html")
    } else {
        root.join(rel)
    };

    let (status, body, ct) = if file_path.is_file() {
        let body = std::fs::read(&file_path).unwrap_or_default();
        let ext = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let ct = match ext {
            "html" => "text/html; charset=utf-8",
            "js" => "application/javascript",
            "css" => "text/css",
            "json" | "webmanifest" => "application/json",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "avif" => "image/avif",
            _ => "application/octet-stream",
        };
        ("200 OK", body, ct)
    } else {
        ("404 Not Found", b"Not Found".to_vec(), "text/plain")
    };

    let header = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: {ct}\r\n\
         Content-Length: {}\r\n\
         Connection: close\r\n\
         \r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(&body);
}

// ===========================================================================
// Setup helpers (mirrors browser_layout.rs)
// ===========================================================================

fn generated_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/browser/generated")
}

fn ensure_fixtures_built() {
    static BUILT: OnceLock<()> = OnceLock::new();
    BUILT.get_or_init(|| {
        let bin = env!("CARGO_BIN_EXE_simple-gal");
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let status = Command::new(bin)
            .args([
                "build",
                "--source",
                root.join("fixtures/browser-content").to_str().unwrap(),
                "--output",
                root.join("tests/browser/generated").to_str().unwrap(),
                "--temp-dir",
                root.join(".simple-gal-browser-temp").to_str().unwrap(),
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

fn start_server() -> TestServer {
    ensure_fixtures_built();
    TestServer::start(generated_dir())
}

/// Wait for the service worker to reach the `activated` state.
/// Panics after 10 s if the SW never activates (install/activate failed).
fn wait_for_sw(tab: &Tab) {
    tab.evaluate(
        r#"Promise.race([
            navigator.serviceWorker.ready.then((reg) => {
                const sw = reg.active;
                if (sw && sw.state === 'activated') return 'ok';
                return new Promise((resolve) => {
                    sw.addEventListener('statechange', () => {
                        if (sw.state === 'activated') resolve('ok');
                    });
                });
            }),
            new Promise((_, reject) =>
                setTimeout(() => reject('SW activation timeout (10 s)'), 10000)
            ),
        ])"#,
        true,
    )
    .expect("service worker failed to activate");
}

// ===========================================================================
// First load (no cache)
// ===========================================================================

#[test]
#[ignore]
fn sw_activates_on_first_load() {
    let server = start_server();
    let tab = browser().new_tab().unwrap();
    tab.navigate_to(&server.url())
        .unwrap()
        .wait_until_navigated()
        .unwrap();

    wait_for_sw(&tab);
}

#[test]
#[ignore]
fn sw_caches_core_assets_on_install() {
    let server = start_server();
    let tab = browser().new_tab().unwrap();
    tab.navigate_to(&server.url())
        .unwrap()
        .wait_until_navigated()
        .unwrap();
    wait_for_sw(&tab);

    let version = env!("CARGO_PKG_VERSION");
    let js = format!(
        r#"(async () => {{
            const cache = await caches.open('simple-gal-v{version}');
            const keys = await cache.keys();
            return JSON.stringify(keys.map(r => new URL(r.url).pathname));
        }})()"#
    );
    let result = tab.evaluate(&js, true).unwrap();
    let urls: Vec<String> = serde_json::from_str(result.value.unwrap().as_str().unwrap()).unwrap();

    assert!(
        urls.contains(&"/index.html".to_string()),
        "should cache /index.html, got: {urls:?}"
    );
    assert!(
        urls.contains(&"/offline.html".to_string()),
        "should cache /offline.html, got: {urls:?}"
    );
    assert!(
        urls.contains(&"/site.webmanifest".to_string()),
        "should cache /site.webmanifest, got: {urls:?}"
    );
}

// ===========================================================================
// Second load (from cache — SW controls the page)
// ===========================================================================

#[test]
#[ignore]
fn sw_controls_page_after_reload() {
    let server = start_server();
    let tab = browser().new_tab().unwrap();

    // First load — registers the SW
    tab.navigate_to(&server.url())
        .unwrap()
        .wait_until_navigated()
        .unwrap();
    wait_for_sw(&tab);

    // Reload — SW should now intercept fetches
    tab.navigate_to(&server.url())
        .unwrap()
        .wait_until_navigated()
        .unwrap();
    thread::sleep(Duration::from_millis(300));

    let controlled = tab
        .evaluate("!!navigator.serviceWorker.controller", false)
        .unwrap()
        .value
        .unwrap()
        .as_bool()
        .unwrap();
    assert!(controlled, "SW should control page after reload");
}

// ===========================================================================
// Stale-while-revalidate strategy
// ===========================================================================

#[test]
#[ignore]
fn sw_stale_while_revalidate_caches_and_serves() {
    let server = start_server();
    let tab = browser().new_tab().unwrap();

    // First load + wait for SW
    tab.navigate_to(&server.url())
        .unwrap()
        .wait_until_navigated()
        .unwrap();
    wait_for_sw(&tab);

    // Reload so SW controls the page
    tab.navigate_to(&server.url())
        .unwrap()
        .wait_until_navigated()
        .unwrap();
    thread::sleep(Duration::from_millis(300));

    // Fetch site.webmanifest through the SW (hits the SWR path)
    let ok = tab
        .evaluate("fetch('/site.webmanifest').then(r => r.ok)", true)
        .unwrap()
        .value
        .unwrap()
        .as_bool()
        .unwrap();
    assert!(ok, "first SWR fetch should succeed");

    // Wait for background cache.put to complete
    thread::sleep(Duration::from_millis(500));

    // Verify the asset was cached by SWR
    let version = env!("CARGO_PKG_VERSION");
    let js = format!(
        r#"(async () => {{
            const cache = await caches.open('simple-gal-v{version}');
            return !!(await cache.match('/site.webmanifest'));
        }})()"#
    );
    let cached = tab
        .evaluate(&js, true)
        .unwrap()
        .value
        .unwrap()
        .as_bool()
        .unwrap();
    assert!(cached, "site.webmanifest should be cached after SWR fetch");

    // Second fetch — served from cache (stale), revalidated in background
    let ok2 = tab
        .evaluate("fetch('/site.webmanifest').then(r => r.ok)", true)
        .unwrap()
        .value
        .unwrap()
        .as_bool()
        .unwrap();
    assert!(ok2, "second SWR fetch (from cache) should succeed");
}

// ===========================================================================
// Error responses must NOT be cached
// ===========================================================================

#[test]
#[ignore]
fn sw_does_not_cache_error_responses() {
    let server = start_server();
    let tab = browser().new_tab().unwrap();

    // First load + SW activation + reload for control
    tab.navigate_to(&server.url())
        .unwrap()
        .wait_until_navigated()
        .unwrap();
    wait_for_sw(&tab);
    tab.navigate_to(&server.url())
        .unwrap()
        .wait_until_navigated()
        .unwrap();
    thread::sleep(Duration::from_millis(300));

    // Fetch a path that returns 404 — goes through SWR
    tab.evaluate("fetch('/does-not-exist-xyz').catch(() => null)", true)
        .unwrap();
    thread::sleep(Duration::from_millis(500));

    // Verify the 404 was NOT cached
    let version = env!("CARGO_PKG_VERSION");
    let js = format!(
        r#"(async () => {{
            const cache = await caches.open('simple-gal-v{version}');
            return !!(await cache.match('/does-not-exist-xyz'));
        }})()"#
    );
    let cached = tab
        .evaluate(&js, true)
        .unwrap()
        .value
        .unwrap()
        .as_bool()
        .unwrap();
    assert!(
        !cached,
        "404 responses should NOT be cached (response.ok guard)"
    );
}
