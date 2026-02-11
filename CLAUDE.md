# Simple Gal

A minimal static site generator for fine art photography portfolios.

## Build Pipeline

Three-stage pipeline: **scan** (filesystem → manifest) → **process** (responsive images + thumbnails) → **generate** (HTML site with inline CSS).

```bash
cargo run -- build --source content --output dist
```

## Testing

```bash
cargo test                                          # unit + integration tests
cargo test --test browser_layout -- --ignored       # browser layout tests (needs Chrome)
```

**Principles:** unit-test first; all pipeline logic is pure functions on data — no integration tests needed for correctness. Browser tests for CSS layout only. See `docs/dev/testing.md` for the full guide.

**Shared fixtures:** `fixtures/content/` exercises the full feature set (config chain, sidecars, markdown priority, nested albums, pages, link pages, hidden dirs, assets directory). Tests copy it to a temp dir via `setup_fixtures()`.

**Test helpers:** `src/test_helpers.rs` — lookup helpers (`find_album`, `find_image`, `find_page`), bulk extractors (`album_titles`, `image_titles`), nav assertions (`assert_nav_shape`). Use these instead of writing ad-hoc fixture setup.

**Browser tests:** `tests/browser_layout.rs` — headless Chrome via `headless_chrome` crate. Uses real HTML from `fixtures/browser-content/`.
