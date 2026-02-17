//! # Simple Gal
//!
//! A minimal static site generator for fine art photography portfolios.
//! Your filesystem is the data source: directories become albums, images are
//! ordered by numeric prefix, and markdown files become pages.
//!
//! # Architecture: Three-Stage Pipeline
//!
//! Simple Gal processes content through three independent stages, each producing
//! a JSON manifest that the next stage consumes:
//!
//! ```text
//! 1. Scan      content/  →  manifest.json    (filesystem → structured data)
//! 2. Process   manifest  →  processed/       (responsive sizes + thumbnails)
//! 3. Generate  manifest  →  dist/            (final HTML site)
//! ```
//!
//! This separation exists for three reasons:
//!
//! - **Debuggability**: each manifest is human-readable JSON you can inspect.
//! - **Incremental builds**: skip stages whose inputs haven't changed.
//! - **Testability**: each stage is a pure function from manifest to manifest,
//!   so unit tests can exercise pipeline logic without touching the filesystem
//!   or encoding images.
//!
//! # Module Map
//!
//! | Module | Role |
//! |--------|------|
//! | [`scan`] | Stage 1 — walks the content directory, extracts metadata, produces the scan manifest |
//! | [`process`] | Stage 2 — generates responsive AVIF images and thumbnails from the scan manifest |
//! | [`generate`] | Stage 3 — renders the final HTML site from the process manifest using Maud |
//! | [`config`] | Hierarchical `config.toml` loading, validation, merging, and CSS generation |
//! | [`types`] | Shared types serialized between stages (`NavItem`, `Page`) |
//! | [`naming`] | `NNN-name` filename convention parser used by all entry types |
//! | [`metadata`] | Image metadata resolution: IPTC tags, sidecar files, filename fallback |
//! | [`imaging`] | Pure-Rust image operations: resize, thumbnail, IPTC parsing |
//! | [`output`] | CLI output formatting — tree-based display of pipeline results |
//!
//! # Design Decisions
//!
//! ## AVIF-Only Output
//!
//! All generated images are AVIF. The format has had [100% browser support since
//! September 2023](https://caniuse.com/avif) and produces dramatically smaller
//! files than JPEG at equivalent quality. Using a single modern format avoids the
//! complexity of multi-format `<picture>` fallbacks and keeps the output directory
//! clean.
//!
//! ## Maud Over Template Engines
//!
//! HTML is generated with [Maud](https://maud.lambda.xyz/), a compile-time HTML
//! macro system, rather than Handlebars or Tera. Advantages:
//!
//! - **Compile-time checking**: malformed HTML is a build error, not a runtime surprise.
//! - **Type-safe**: template variables are Rust expressions — no stringly-typed lookups.
//! - **XSS-safe by default**: all interpolation is auto-escaped.
//! - **Zero runtime files**: no template directory to ship or get out of sync.
//!
//! ## Pure-Rust Imaging (No ImageMagick, No FFmpeg)
//!
//! The [`imaging`] module uses the `image` crate (Lanczos3 resampling) and `rav1e`
//! (AVIF encoding) — both pure Rust. This eliminates system dependencies entirely:
//! no `apt install`, no Homebrew, no version conflicts. The binary is fully
//! self-contained, which is critical to the "forever stack" premise — a user can
//! download a single binary and it just works, on any machine, indefinitely.
//!
//! ## Config Cascading (Root → Group → Gallery)
//!
//! Configuration files at any level of the directory tree override their parent:
//!
//! ```text
//! content/config.toml           ← root (overrides stock defaults)
//! content/Travel/config.toml    ← group (overrides root)
//! content/Travel/Japan/config.toml ← gallery (overrides group)
//! ```
//!
//! Photographers want per-gallery control over aspect ratios, quality, and theme
//! settings without repeating the entire config. The merge logic lives in
//! [`config::SiteConfig::merge`].
//!
//! ## NNN-Prefix Ordering
//!
//! Directories and files use a numeric prefix (`001-`, `020-`, etc.) for explicit
//! ordering. This is parsed by [`naming::parse_entry_name`]. Items without a prefix
//! are processed but hidden from navigation — useful for work-in-progress content
//! that should remain accessible by direct URL. The filesystem is the source of
//! truth; no database, no front-matter, no separate ordering file.
//!
//! ## Stale-While-Revalidate Service Worker
//!
//! Every generated site is a PWA with a service worker using a stale-while-revalidate
//! caching strategy. This gives visitors instant loads from cache while transparently
//! fetching fresh content in the background. The cache is versioned by the build
//! version string, so deploying a new build automatically invalidates old caches.
//!
//! # The "Forever Stack"
//!
//! Simple Gal is designed to be usable decades from now with minimal fuss. The
//! output is plain HTML, established CSS, and ~30 lines of vanilla JavaScript.
//! The binary has zero runtime dependencies. AVIF is an ISO standard. The generated
//! site can be dropped on any file server — no Node, no PHP, no database. If a
//! browser can render HTML, it can display your portfolio.

pub mod config;
pub mod generate;
pub mod imaging;
pub mod metadata;
pub mod naming;
pub mod output;
pub mod process;
pub mod scan;
pub mod types;

#[cfg(test)]
pub(crate) mod test_helpers;
