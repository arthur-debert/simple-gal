//! Image processing — pure Rust, zero external dependencies.
//!
//! | Operation | Crate / function |
//! |---|---|
//! | **Identify** | `image::image_dimensions` |
//! | **IPTC metadata** | custom parser (JPEG APP13 + TIFF IFD) |
//! | **Resize → AVIF** | Lanczos3 + rav1e encoder |
//! | **Thumbnail** | `resize_to_fill` + `unsharpen` |
//!
//! The module is split into:
//! - **Calculations**: Pure functions for dimension math (unit testable)
//! - **Parameters**: Data structures describing image operations
//! - **Backend**: [`ImageBackend`] trait + [`RustBackend`]
//! - **Operations**: High-level functions combining calculations + backend

pub mod backend;
mod calculations;
pub(crate) mod iptc_parser;
pub mod operations;
mod params;
pub mod rust_backend;

pub use backend::{BackendError, ImageBackend};
pub use rust_backend::RustBackend;
// Re-exported for tests (process.rs, operations.rs tests use this)
#[cfg(test)]
pub use backend::Dimensions;
pub use operations::{
    ResponsiveConfig, ThumbnailConfig, create_responsive_images, create_thumbnail, get_dimensions,
};
pub use params::{Quality, Sharpening};
