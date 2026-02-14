# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
- AVIF and WebP format support
- Hierarchical configuration system via `config.toml`
- `gen-config` command for stock config generation
- Album descriptions from `description.txt` with markdown support
- Image metadata extraction from EXIF and sidecar files
- `NNN-name` convention for ordering albums and images
- Configurable thread pool for parallel image processing
- Clean directory-based URLs for albums and images
- GitHub Pages deployment workflow
