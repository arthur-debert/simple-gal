//! Parameter types for image operations.
//!
//! These structs describe *what* to do, not *how* to do it. They are the
//! interface between the high-level [`operations`](super::operations) module
//! (which decides what images to create) and the [`backend`](super::backend)
//! (which does the actual pixel work). This separation allows swapping backends
//! (e.g. for testing with a mock) without changing operation logic.
//!
//! ## Types
//!
//! - [`Quality`] — Lossy encoding quality (1–100, default 90). Clamped on construction.
//! - [`Sharpening`] — Unsharp-mask parameters (sigma + threshold) for thumbnail crispness.
//! - [`ResizeParams`] — Full specification for a resize: source, output path, target dimensions, quality.
//! - [`ThumbnailParams`] — Full specification for a thumbnail: source, output, crop dimensions, quality, optional sharpening.

use std::path::PathBuf;

/// Quality setting for lossy image encoding (1-100).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quality(pub u32);

impl Quality {
    pub fn new(value: u32) -> Self {
        Self(value.clamp(1, 100))
    }

    pub fn value(self) -> u32 {
        self.0
    }
}

impl Default for Quality {
    fn default() -> Self {
        Self(90)
    }
}

/// Sharpening parameters for unsharp mask.
///
/// - `sigma`: Standard deviation of the Gaussian blur (higher = more sharpening)
/// - `threshold`: Minimum brightness difference to sharpen (0 = sharpen all pixels)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sharpening {
    pub sigma: f32,
    pub threshold: i32,
}

impl Sharpening {
    /// Light sharpening suitable for thumbnails.
    pub fn light() -> Self {
        Self {
            sigma: 0.5,
            threshold: 0,
        }
    }
}

/// Parameters for a simple resize operation.
#[derive(Debug, Clone, PartialEq)]
pub struct ResizeParams {
    pub source: PathBuf,
    pub output: PathBuf,
    pub width: u32,
    pub height: u32,
    pub quality: Quality,
}

/// Parameters for a thumbnail operation (resize + center crop).
#[derive(Debug, Clone, PartialEq)]
pub struct ThumbnailParams {
    pub source: PathBuf,
    pub output: PathBuf,
    /// Final crop dimensions.
    pub crop_width: u32,
    pub crop_height: u32,
    pub quality: Quality,
    pub sharpening: Option<Sharpening>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_clamps_to_valid_range() {
        assert_eq!(Quality::new(0).value(), 1);
        assert_eq!(Quality::new(50).value(), 50);
        assert_eq!(Quality::new(150).value(), 100);
    }

    #[test]
    fn quality_default_is_90() {
        assert_eq!(Quality::default().value(), 90);
    }

    #[test]
    fn sharpening_light_values() {
        let s = Sharpening::light();
        assert_eq!(s.sigma, 0.5);
        assert_eq!(s.threshold, 0);
    }
}
