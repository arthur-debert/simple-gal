# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
