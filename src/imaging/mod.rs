//! Image processing abstraction layer.
//!
//! Two backends are available, selectable via `[backend]` in `config.toml`:
//!
//! | | `"imagemagick"` (default) | `"rust"` |
//! |---|---|---|
//! | **Identify** | `identify -format` | `image::image_dimensions` |
//! | **IPTC metadata** | `identify -format %[IPTC:*]` | custom parser (JPEG APP13 + TIFF IFD) |
//! | **Resize → WebP** | `convert -resize` | Lanczos3 + `webp` crate (vendored libwebp) |
//! | **Resize → AVIF** | `convert -resize -define heic:speed=6` | Lanczos3 + rav1e encoder |
//! | **Thumbnail** | `convert -resize^ -extent -sharpen` | `resize_to_fill` + `unsharpen` |
//!
//! Both backends have **full parity** — every operation produces identical output
//! dimensions and supports the same quality/sharpening parameters. The cross-backend
//! dimension parity test (`tests/compare_backends.rs`) enforces this.
//!
//! To switch to the pure Rust backend (zero external dependencies):
//!
//! ```toml
//! [backend]
//! name = "rust"
//! ```
//!
//! The module is split into:
//! - **Calculations**: Pure functions for dimension math (unit testable)
//! - **Parameters**: Data structures describing image operations
//! - **Backend**: [`ImageBackend`] trait + [`ImageMagickBackend`] / [`RustBackend`]
//! - **Operations**: High-level functions combining calculations + backend

pub mod backend;
mod calculations;
pub(crate) mod iptc_parser;
pub mod operations;
mod params;
pub mod rust_backend;

pub use backend::{BackendError, ImageBackend, ImageMagickBackend};
pub use rust_backend::RustBackend;
// Re-exported for tests (process.rs, operations.rs tests use this)
#[cfg(test)]
pub use backend::Dimensions;
pub use operations::{
    ResponsiveConfig, ThumbnailConfig, create_responsive_images, create_thumbnail, get_dimensions,
};
pub use params::{Quality, Sharpening};
