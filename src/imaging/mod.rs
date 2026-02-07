//! Image processing abstraction layer.
//!
//! This module provides a clean separation between:
//! - **Calculations**: Pure functions for dimension math (unit testable)
//! - **Parameters**: Data structures describing image operations
//! - **Backend**: Trait + implementation for actual image processing
//! - **Operations**: High-level functions combining calculations + backend

pub mod backend;
mod calculations;
pub(crate) mod iptc_parser;
pub mod operations;
mod params;
#[allow(dead_code)]
pub mod rust_backend;

pub use backend::{BackendError, ImageBackend, ImageMagickBackend};
// Re-exported for tests (process.rs, operations.rs tests use this)
#[cfg(test)]
pub use backend::Dimensions;
pub use operations::{
    ResponsiveConfig, ThumbnailConfig, create_responsive_images, create_thumbnail, get_dimensions,
};
pub use params::{Quality, Sharpening};
