# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Open Graph + Twitter Card link previews on the home page, container/gallery-list pages, album pages, and image pages. When a link is pasted into WhatsApp, iMessage, Slack, Discord, Facebook, or X, the preview shows the gallery's cover image, the page title, and a breadcrumb description (e.g. `Gallery › Travel › Japan › 1. tokyo`). Breadcrumb descriptions matter for photography sites where individual photos often have no title or caption — a generic "Photo from the gallery" would be uninformative, but the crumb tells the reader exactly where in the gallery they're about to land.
- New optional `base_url` field in `SiteConfig` (e.g. `base_url = "https://gallery.example.com"`). Required because `og:image` and `og:url` MUST be absolute URLs for scrapers like WhatsApp and iMessage to resolve them; there is no safe relative-URL fallback. When `base_url` is unset, **no** OG/Twitter meta tags are emitted — the site still builds and works, it just won't produce rich link previews. No other config or behavior changes when `base_url` is omitted, so this is a zero-cost opt-in.
- OG image variant selection picks the smallest responsive variant ≥ 1200px wide (Facebook/Twitter's recommended 1.91:1 canvas width), falling back to the largest available variant when every generated size is smaller. 1200 also keeps the file comfortably under WhatsApp's ~300KB preview-image budget for AVIF encodes at normal quality.

### Fixed
- Broken `<img src>` and `<srcset>` URLs on nested album pages (paths containing a `/`, e.g. `travel/japan/`). `render_album_page` and `render_image_page` stripped only the last path segment from the stored root-relative image paths, so a page at `/travel/japan/` emitted `src="travel/japan/001-tokyo-thumb.avif"` which browsers resolved to `/travel/japan/travel/japan/001-tokyo-thumb.avif` — a 404. Both renderers now strip the full album path, so nested album pages render with bare filenames (`src="001-tokyo-thumb.avif"`) and nested image pages with single-hop relative paths (`srcset="../001-tokyo-1.avif 1w"`). Flat albums were never affected. (#56)
- Test fixture `create_nested_test_album` stored image paths in a parent-relative shape (`"Night/..."`) that happened to match the buggy strip logic, so the regression above slipped through CI. Fixture now uses the full root-relative shape (`"NY/Night/..."`) the process stage actually emits, and the two nested-album assertions were tightened to fail loudly if a doubled album prefix ever shows up in rendered URLs again.

## [0.16.0] - 2026-04-12

### Added
- `--format ndjson` output mode: newline-delimited JSON streaming. Each line is a self-contained JSON object with a `"type"` discriminator. During `process` and `build`, progress events (`album_started`, `image_processed`, `cache_pruned`) stream to stdout as they happen — one compact JSON line per event, tagged `"type": "progress"`. The final line is the result envelope tagged `"type": "result"`, identical in shape to `--format json`. Error envelopes on stderr are also compact single-line in NDJSON mode. Commands without streaming progress (`scan`, `check`, `generate`, `config`) emit a single `"type": "result"` line. This lets GUIs and scripts show incremental progress without waiting for the full pipeline to finish.
- `--format progress` output mode: structured progress stream for GUI progress bars. Emits NDJSON lines with pre-computed `percent` (0–100), `stage` (`scan`/`process`/`generate`), and `images_done`/`images_total`/`variants_done`/`variants_total` counters. Weight model: scan=2%, process=90%, generate=8%. Within process, each image variant (responsive size or thumbnail) is one unit of progress. The `build` command streams one progress line per completed image; other commands emit a single result line. Variant totals are estimated from the config (`images.sizes` count + thumbnail + optional full-index thumbnail).

## [0.15.0] - 2026-04-12

### Added
- `simple-gal config` subcommand group, owned end-to-end by [`clapfig`](https://crates.io/crates/clapfig) 0.15: `config gen` (commented TOML template auto-derived from `SiteConfig` doc comments + `#[config(default = ...)]` annotations), `config schema` (JSON Schema, Draft 2020-12, intended for GUI form generators), `config list` (flat dotted-key view of the resolved config), `config get KEY` (single key + doc comment), and `config set KEY VALUE` / `config unset KEY` placeholders that error with a clear `NoPersistPath` until a persist scope is wired. All variants honor `--format json` via the new `ConfigOpPayload` envelope (`{ok, command: "config", data: {action, ...}}`), so automation can consume any of them without scraping text.
- `--source content config schema -o site.schema.json` writes the JSON Schema to a file. The schema includes per-field `default`, `description` (from doc comments), `type`, and `additionalProperties: false` on every nested object.

### Changed
- **`SiteConfig` migrated to [`confique::Config`](https://crates.io/crates/confique).** Defaults now live as `#[config(default = ...)]` on the struct fields, sparse loading + deep merge are handled by confique's generated `Layer` type, and the per-directory cascade in `scan.rs` threads `SiteConfigLayer` instead of resolved `SiteConfig`s, folding layers via `Layer::with_fallback` and finalizing only at album leaves. Net delete: ~430 lines of `PartialSiteConfig`, hand-written `merge()` methods, and the hand-rolled `stock_config_toml()` template string.
- A custom `Deserialize` impl on `SiteConfig` routes any direct deserialize (manifest reads in `process.rs`, JSON test fixtures, anything calling `serde_json::from_str::<SiteConfig>`) through the same layer + defaults pipeline, so a sparse `"config": {}` in a manifest still produces a fully-populated config.
- `simple-gal gen-config` removed in favor of `simple-gal config gen`. The new template is auto-generated from confique struct metadata, so it can no longer drift from the code.
- `--output` is no longer a global flag — only `build` and `generate` use it, and marking it global collided with clapfig's `config gen --output` / `config schema --output`. `--source` and `--temp-dir` are still global.
- `ColorScheme` was split into distinct `LightColors` / `DarkColors` types, and `ClampSize` into `MatX` / `MatY`. confique nested defaults come from the inner type, so two instances of one type can't have different defaults; splitting is the only way to keep accurate per-side defaults visible in both the schema and the generated template.
- Semantic validation (`quality ≤ 100`, non-zero aspect ratios, non-empty `images.sizes`) now runs through clapfig's `.post_validate()` hook on every `simple-gal config <action>` invocation. The standalone `SiteConfig::validate()` method stays for the per-directory cascade in `scan.rs`.
- `clapfig` bumped from 0.13 → 0.15. Adds `confique = "0.4"` as a direct dependency (used for the `Config` derive macro and `Layer` trait).

## [0.14.0] - 2026-04-11

### Added
- Machine-readable JSON output for every command, gated by a new global `--format {text,json}` flag. `scan` keeps its JSON default (from v0.12); every other command defaults to text. In JSON mode each command emits exactly one tagged envelope — `{"ok": true, "command": "<name>", "data": {...}}` — to stdout on success, so automation (GUIs, shell scripts) can parse output without scraping.
- Structured error envelopes on stderr when a command fails in JSON mode: `{"ok": false, "kind": "<classification>", "message": "...", "causes": [...], "config": {path, line, column, snippet}?}`. Config parse failures populate the `config` field with the same snippet/line/column information clapfig shows in text mode, so a GUI can highlight the exact offending token without re-parsing the TOML.
- Granular process exit codes that let callers branch on failure type without parsing messages: `0` success, `1` internal, `2` usage (clap), `3` config, `4` io, `5` scan, `6` process, `7` generate, `8` validation. Previously every failure exited `1`.
- Global `--quiet` flag suppresses non-error stdout in text mode (no effect in JSON mode, which is already a single document).

### Changed
- Text-mode error rendering is unchanged (clapfig rich/plain for config errors, plain `Error:` + cause chain otherwise). JSON-mode error rendering replaces it with the structured envelope on stderr; stdout stays empty on failure so scripts can always `jq` stderr.
- `--format` moved from a `scan`-only flag to a global flag. `simple-gal scan --format json` still works; `simple-gal --format json scan` is now the canonical form and the same flag applies to every other command.

## [0.13.0] - 2026-04-11

### Changed
- Config file errors now render through [`clapfig`](https://crates.io/crates/clapfig) instead of dumping the raw `toml::de::Error` Debug struct. Parse failures show a header, the file path, the offending line as a source snippet, and a caret pointing at the exact token — e.g. an unquoted `thumbnail_gap = 0.1rem` now points at `0.1rem` with `expected newline, \`#\`` as the label. The CLI picks clapfig's `render_rich` (miette-based, colored, Unicode box drawing) when stderr is a TTY and `render_plain` (ANSI-free, pipe-safe) otherwise. `ConfigError::Toml` now carries `path` + `source_text` alongside the underlying parser error so the renderer has everything it needs.

## [0.12.0] - 2026-04-11

### Changed
- `scan` command now outputs JSON to stdout by default instead of the human-readable tree — use `--format text` for the previous behavior
- `scan` no longer saves `manifest.json` to the temp directory by default — use `--save-manifest` to opt in (defaults to `<temp-dir>/manifest.json`, or pass a custom path)

### Added
- `--format` flag on `scan` command: `json` (default) for machine-readable output, `text` for human-readable tree display
- `--save-manifest [path]` flag on `scan` command: explicitly save the JSON manifest to disk
- Site-wide "All Photos" page: a single thumbnail grid containing every image from every public (numbered) album across the site. Opt-in via the new `[full_index]` config section, off by default:
  - `generates` — render `/all-photos/` when `true`
  - `show_link` — add an "All Photos" entry to the navigation menu (only surfaced when `generates` is also `true`, to avoid dangling links)
  - `thumb_ratio` — aspect ratio `[width, height]` for full-index thumbnails, independent of the per-album `[thumbnails]` ratio
  - `thumb_size` — short-edge size in pixels for full-index thumbnails
  - `thumb_gap` — CSS gap between thumbnails on the All Photos grid
  - Each full-index thumbnail links back to the image's normal page. Full-index thumbnails are cached separately from album thumbnails via a distinct params hash, so both sets coexist without collisions.

## [0.11.7] - 2026-03-26

### Added
- Arrow Up and Escape keys navigate up one level: from a photo to its album, from an album to its parent container, no-op on the home page
- Vim-style `j`/`k` keys for next/previous image navigation (alongside existing `l`/`h` and arrow keys)
- Keyboard navigation (up-level, vim keys) now works on all page types, not just image pages

## [0.11.6] - 2026-03-25

### Fixed
- Responsive `srcset` used the target size (longer edge) as the `w` descriptor instead of the actual pixel width — portrait images reported inflated widths, causing browsers to select too-small variants on desktop viewports
- Responsive `sizes` attribute was a hardcoded `80vw` that ignored aspect ratio — portrait images on wide screens are height-constrained and display much narrower than 80vw, causing mismatched browser selection. Now computed per-image from aspect ratio with a pixel cap at the largest generated width

### Changed
- Responsive image generation now caps at source dimensions instead of silently dropping sizes larger than the original — a 1800px source with configured sizes `[800, 1400, 2080]` now produces `[800, 1400, 1800]` instead of `[800, 1400]`, ensuring the browser always has the full-resolution variant available

## [0.11.4] - 2026-02-25

### Changed
- Default `thumbnail_gap` reduced from `1rem` to `0.2rem` for tighter grid spacing

## [0.11.3] - 2026-02-23

## [0.11.2] - 2026-02-23

### Changed
- URL slugs are now normalized: lowercased with underscores and spaces replaced by hyphens. For example, `Magna Graecia With Theo` becomes `magna-graecia-with-theo` instead of `Magna%20Graecia%20With%20Theo`. Affects album paths, image page URLs, and page slugs.

### Fixed
- Race condition in content-addressed cache: when images swap positions (e.g. reordering photos), parallel processing threads could clobber each other's cached files, causing two gallery positions to show the same image. The cache mutex now spans the entire find+copy+insert sequence, and `insert` invalidates stale content-index entries for displaced content.
- Cache now prunes stale entries after each build: processed files for deleted/renumbered images and renamed albums are removed instead of accumulating indefinitely.
- Nested album cache paths used only the leaf directory name (`Japan/`) instead of the full relative path (`Travel/Japan/`), causing incorrect cache lookups for nested albums.

## [0.11.1] - 2026-02-23

## [0.11.0] - 2026-02-21

### Added
- Nested gallery support: container directories (directories with sub-albums but no images) now generate their own gallery-list pages showing thumbnail cards for each child album/container
- `description` field on `NavItem` — container directories can have `description.md`/`description.txt` shown on their gallery-list page
- Breadcrumbs reflect nesting: album pages show `Home › NY › Snow` instead of `Home › Snow`, and image pages include the full chain

### Changed
- Index page now shows only top-level navigation entries instead of all albums flat — nested albums appear under their container
- Navigation containers are clickable links to their gallery-list page instead of inert `<span>` labels
- Index page and container gallery-list pages share a single rendering path (`render_gallery_list_page`)

### Fixed
- Nested album pages had broken image paths: `strip_prefix` stripped the full nested path instead of just the album directory name, causing double-nested URLs (e.g. `/NY/Night/Night/thumb.avif`)

## [0.10.2] - 2026-02-21

## [0.10.1] - 2026-02-21

### Changed
- CLI output is now information-centric instead of file-path-centric: every entity (album, image, page) leads with its positional index and title, with filesystem paths shown as indented `Source:` lines. Shared display helpers (`entity_header`, `image_line`) enforce consistent formatting across scan, process, and generate stages.
- Process stage output now shows per-variant cache status (`cached`, `copied`, `encoded`) for each responsive size and thumbnail, replacing the previous flat `sizes + thumb` summary.
- `ProcessEvent::ImageProcessed` now carries `index`, `source_path`, and per-variant `VariantInfo` (label + cache status) so callers can display rich progress without coupling to process internals.

### Removed
- `content_root` config key — it was redundant since the config file already lives inside the content directory, so the content root is known by the time the config is found. The `--source` CLI flag is the sole way to specify the content directory.

## [0.10.0] - 2026-02-21

### Changed
- Image processing cache is now content-addressed: cache keys use source file hash + encoding parameters instead of output paths. Album renames, file renumbers, and slug changes no longer invalidate the cache — only actual image content or encoding parameter changes trigger re-encoding. When a cached file is found at a different path (e.g. after an album rename), it is copied instead of re-encoded.

## [0.9.0] - 2026-02-21

### Changed
- Thumb-designated images (`NNN-thumb.<ext>`) are no longer included as browsable gallery images — they are now used exclusively as the album's representative thumbnail on the index page

## [0.8.4] - 2026-02-21

### Fixed
- View transitions broken with PWA: service worker's `respondWith()` on navigation requests interfered with the CSS View Transitions API (`@view-transition { navigation: auto }`), causing abrupt page swaps instead of smooth fades when images weren't cached. Navigation requests now pass through to the browser natively.
- Navigation click zones (prev/next) now overlap 20% of the image and extend to the page edges, instead of using a fixed 30% viewport width. On wide screens with portrait images, the old zones were entirely in the mat area and hard to find; the zones now always start at the image edges.

### Removed
- Offline fallback page (`offline.html`) — no longer generated since navigation requests are not intercepted by the service worker

## [0.8.3] - 2026-02-20

### Changed
- Image processing now streams progress to the terminal as each image completes, instead of waiting for the entire stage to finish before printing output. Uses an `mpsc` channel from the process module to a printer thread, preserving the separation between processing logic and output formatting.

## [0.8.2] - 2026-02-20

### Fixed
- AVIF files with size-0 `mdat` boxes (common in Lightroom/modern encoders) now parse correctly — works around an `avif-parse` limitation by patching the ISOBMFF header in memory
- `--source` with absolute/cross-project paths no longer silently scans the wrong directory — relative `content_root` is now resolved against the source path instead of the current working directory

### Changed
- Upgraded `avif-parse` from 1.x to 2.0.0

## [0.8.1] - 2026-02-20

### Fixed
- Release CI: bump Rust toolchain from 1.90.0 to 1.93.1 to fix `cross` requiring rustc 1.92.0+

## [0.8.0] - 2026-02-20

### Added
- AVIF source image support via pure Rust decoder (`avif-parse` + `rav1d`), replacing the broken `image` crate AVIF decode path

### Fixed
- Image format list now derived from actual decode capabilities instead of a hardcoded list

## [0.7.0] - 2026-02-20

### Added
- Custom album thumbnail via naming convention: name an image `NNN-thumb.<ext>` (or `NNN-thumb-Title.<ext>`) to designate it as the album's representative thumbnail instead of the default first image
- AVIF source image support: `.avif` files are now accepted as input alongside JPEG, PNG, TIFF, and WebP

## [0.6.0] - 2026-02-17

### Added
- Image processing cache: repeated builds skip AVIF encoding for unchanged images, making incremental builds near-instant (6s → 0.2s for 3 images). Cache keys are SHA-256 of source content + encoding parameters (size, quality, aspect ratio), so config changes automatically invalidate.
- `--no-cache` flag on `build` and `process` commands to force full re-encoding

## [0.5.0] - 2026-02-17

### Added
- Site description on the index page: a `site.md` (or `.txt`) file in the content root is rendered on the home page with the site title, using the same expandable description pattern as album pages
- `site_description_file` config option to customize the description filename (default: `"site"`)
- Desktop 2-column layout for index page when a site description is present (description sidebar + album grid)

### Fixed
- Photo view scrolling broken when description present: invisible click-navigation zones (prev/next) were siblings of `<main>`, intercepting scroll events over 60% of the viewport — moved them inside `<main>` so scroll events propagate to the scrollable container

## [0.4.2] - 2026-02-16

### Added
- `check` command: validate content directory without building (`simple-gal check`)

## [0.4.1] - 2026-02-16

### Fixed
- Service worker: comprehensive error handling across all fetch strategies
  - Added `.catch()` to stale-while-revalidate background fetch to prevent unhandled promise rejections
  - Added `response.ok` guard before all `cache.put()` calls so error responses (404, 500) are never cached
  - Added fallback `Response` objects in all catch paths so `respondWith()` never receives `undefined`
  - Added outer `.catch()` on the image cache-first handler for network failures with empty cache
  - Added cross-origin guard to skip non-same-origin requests
  - Fixed navigation handler to clone response immediately before fire-and-forget cache operation

### Added
- Browser integration tests for service worker lifecycle (`tests/browser_sw.rs`)
  - Tests SW activation, page control after reload, core asset caching, stale-while-revalidate strategy, and error response rejection
  - Uses a minimal TCP-based HTTP server (service workers require HTTP, not `file://`)

## [0.4.0] - 2026-02-16

### Added
- Structured CLI output: tree-based formatting for scan, process, and generate stages
  - Albums shown with positional indices, photo counts, source directories, and truncated descriptions
  - Process output shows generated image sizes per photo
  - Generate output shows output file paths per album and image page
  - Navigation tree walked for consistent hierarchy display
- `source_dir` field on `NavItem` tracking original directory basename
- `support_files` field on `Album` tracking config and description files

### Changed
- Replaced ad-hoc `println!` output across pipeline stages with centralized `output` module
- Build command stage headers now include source/output paths

## [0.3.1] - 2026-02-16

## [0.3.0] - 2026-02-16

### Added
- Custom CSS/JS injection via convention files in `assets/` — zero configuration needed:
  - `custom.css`: linked after main styles for CSS overrides
  - `head.html`: raw HTML injected at end of `<head>` (analytics, meta tags, etc.)
  - `body-end.html`: raw HTML injected before `</body>` (tracking scripts, widgets)

## [0.2.1] - 2026-02-15

### Changed
- Removed WebP output format — all responsive images and thumbnails are now AVIF-only (AVIF has had full browser support since September 2022)
- Simplified image pages from `<picture>` with AVIF/WebP srcsets to plain `<img>` with AVIF srcset
- Removed `webp` crate dependency, eliminating the last C build dependency (libwebp-sys)
- Removed ImageMagick backend — pure Rust image processing only, zero system dependencies
- Removed `[backend]` config section (was selecting between ImageMagick and Rust backends)

### Fixed
- Photo page layout: added bottom mat, description scrolls with photo instead of independently, teaser peeks above nav dots
- Renamed `frame_width` config to `mat` (breaking: update `config.toml` sections from `[theme.frame_x]`/`[theme.frame_y]` to `[theme.mat_x]`/`[theme.mat_y]`)

## [0.2.0] - 2026-02-14

### Added
- Static assets directory: `assets/` contents are copied verbatim to the output root (favicon, fonts, robots.txt, etc.)
- Local font support: `source` field in `[font]` config generates `@font-face` CSS instead of loading from Google Fonts
- Favicon auto-detection: `favicon.ico`, `.svg`, or `.png` in assets directory automatically gets a `<link rel="icon">` tag
- Print view shows "Album › Photo Title" credit line below the image
- PWA support: galleries are installable as home-screen apps for offline-capable, app-like viewing
  - Web App Manifest generated dynamically from `site_title`
  - Service worker with network-first HTML, cache-first images, stale-while-revalidate default
  - Bounded image cache (200 entries, FIFO eviction) to prevent unbounded storage growth
  - Offline fallback page when a requested page isn't cached
  - Default icons (192px, 512px, apple-touch-icon) — overridable via `assets/`
  - Cache versioned to package version for automatic updates on new builds

### Changed
- Replaced click-zone JavaScript with pure HTML/CSS `<a>` overlays, reducing nav.js from ~90 to ~30 lines
- Fixed Escape key navigating to current page instead of album page

### Fixed
- Print view: image disappeared when photo had a description (container query sizing collapsed without fixed viewport height)
- Print view: page split into two pages due to fixed header margin and viewport-height layout

## [0.1.1] - 2026-02-07

### Added
- Release infrastructure: CI workflows, cargo-release, --version flag
- Shared composite action for Rust setup across workflows
- Release workflow with cross-platform binary builds and crates.io publishing
- `--version` flag shows version on release builds, `dev@hash` in development

## [0.1.0] - 2025-01-01

### Added
- Three-stage build pipeline: scan, process, generate
- Responsive image processing with multiple output sizes
- Thumbnail generation with configurable dimensions
- AVIF format support
- Hierarchical configuration system via `config.toml`
- `gen-config` command for stock config generation
- Album descriptions from `description.txt` with markdown support
- Image metadata extraction from EXIF and sidecar files
- `NNN-name` convention for ordering albums and images
- Configurable thread pool for parallel image processing
- Clean directory-based URLs for albums and images
- GitHub Pages deployment workflow
