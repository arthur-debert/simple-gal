//! Image processing — pure Rust, zero external dependencies.
//!
//! This module handles all image manipulation in Simple Gal: reading dimensions,
//! extracting IPTC metadata, generating responsive sizes, and creating thumbnails.
//! Everything uses pure Rust crates (`image`, `rav1e`) — no ImageMagick, no FFmpeg,
//! no system libraries. This is a deliberate choice: the binary is fully self-contained,
//! so it works on any machine without installing prerequisites.
//!
//! ## Operation Table
//!
//! | Operation | Implementation |
//! |---|---|
//! | **Identify** (dimensions) | `image::image_dimensions` |
//! | **IPTC metadata** | Custom parser (`iptc_parser`) — reads JPEG APP13 + TIFF IFD |
//! | **Resize → AVIF** | Lanczos3 resampling + rav1e AVIF encoder |
//! | **Thumbnail** | `resize_to_fill` (center crop) + optional `unsharpen` |
//!
//! ## Architecture: Backend Trait Pattern
//!
//! The module separates *what* to do from *how* to do it using the [`ImageBackend`] trait:
//!
//! - **[`calculations`]** — Pure functions for dimension math (aspect ratios, responsive
//!   sizes). Fully unit-testable with no I/O.
//! - **[`params`]** — Data structs (`ResizeParams`, `ThumbnailParams`) describing operations.
//! - **[`backend`]** — The [`ImageBackend`] trait defining identify/resize/thumbnail.
//!   Includes a `MockBackend` (behind `#[cfg(test)]`) for fast, deterministic tests.
//! - **[`rust_backend`]** — [`RustBackend`], the production implementation using `image` + `rav1e`.
//! - **[`operations`]** — High-level functions (`create_responsive_images`, `create_thumbnail`)
//!   that combine calculations + backend. Accept `&dyn ImageBackend` for testability.

pub mod backend;
pub mod calculations;
pub(crate) mod iptc_parser;
pub mod operations;
pub mod params;
pub mod rust_backend;

#[cfg(test)]
pub use backend::Dimensions;
pub use backend::{BackendError, ImageBackend};
pub use calculations::calculate_thumbnail_dimensions;
pub use operations::{
    ResponsiveConfig, ThumbnailConfig, create_responsive_images, create_thumbnail, get_dimensions,
};
pub use params::{Quality, Sharpening};
pub use rust_backend::RustBackend;
