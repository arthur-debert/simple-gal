# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
