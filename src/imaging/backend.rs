//! Image processing backend trait and shared types.
//!
//! The [`ImageBackend`] trait defines the four operations every backend must
//! support: identify, read_metadata, resize, and thumbnail.
//!
//! The production implementation is
//! [`RustBackend`](super::rust_backend::RustBackend) — pure Rust, zero
//! external dependencies. Everything is statically linked into the binary.

use super::params::{ResizeParams, ThumbnailParams};
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BackendError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Processing failed: {0}")]
    ProcessingFailed(String),
}

/// Result of an identify operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dimensions {
    pub width: u32,
    pub height: u32,
}

/// Embedded image metadata extracted from IPTC fields.
///
/// Field mapping:
/// - `title`: IPTC Object Name (`2:05`) — the "Title" field in Lightroom/Capture One
/// - `description`: IPTC Caption-Abstract (`2:120`) — the "Caption" field in Lightroom
/// - `keywords`: IPTC Keywords (`2:25`) — repeatable field, one entry per keyword
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImageMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub keywords: Vec<String>,
}

/// Trait for image processing backends.
///
/// Every backend must implement all four operations — identify, read_metadata,
/// resize, and thumbnail — so the rest of the codebase is backend-agnostic.
/// See the [module docs](self) for the parity table.
pub trait ImageBackend: Sync {
    /// Get image dimensions.
    fn identify(&self, path: &Path) -> Result<Dimensions, BackendError>;

    /// Read embedded IPTC/EXIF metadata (title, description).
    fn read_metadata(&self, path: &Path) -> Result<ImageMetadata, BackendError>;

    /// Execute a resize operation.
    fn resize(&self, params: &ResizeParams) -> Result<(), BackendError>;

    /// Execute a thumbnail operation (resize + center crop).
    fn thumbnail(&self, params: &ThumbnailParams) -> Result<(), BackendError>;
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::imaging::Sharpening;
    use std::sync::Mutex;

    /// Mock backend that records operations without executing them.
    /// Uses Mutex (not RefCell) so it is Sync and works with rayon's par_iter.
    #[derive(Default)]
    pub struct MockBackend {
        pub identify_results: Mutex<Vec<Dimensions>>,
        pub metadata_results: Mutex<Vec<ImageMetadata>>,
        pub operations: Mutex<Vec<RecordedOp>>,
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum RecordedOp {
        Identify(String),
        ReadMetadata(String),
        Resize {
            source: String,
            output: String,
            width: u32,
            height: u32,
            quality: u32,
        },
        Thumbnail {
            source: String,
            output: String,
            crop_width: u32,
            crop_height: u32,
            quality: u32,
            sharpening: Option<(f32, i32)>,
        },
    }

    impl MockBackend {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn with_dimensions(dims: Vec<Dimensions>) -> Self {
            Self {
                identify_results: Mutex::new(dims),
                metadata_results: Mutex::new(Vec::new()),
                operations: Mutex::new(Vec::new()),
            }
        }

        pub fn with_metadata(dims: Vec<Dimensions>, metadata: Vec<ImageMetadata>) -> Self {
            Self {
                identify_results: Mutex::new(dims),
                metadata_results: Mutex::new(metadata),
                operations: Mutex::new(Vec::new()),
            }
        }

        pub fn get_operations(&self) -> Vec<RecordedOp> {
            self.operations.lock().unwrap().clone()
        }
    }

    impl ImageBackend for MockBackend {
        fn identify(&self, path: &Path) -> Result<Dimensions, BackendError> {
            self.operations
                .lock()
                .unwrap()
                .push(RecordedOp::Identify(path.to_string_lossy().to_string()));

            self.identify_results
                .lock()
                .unwrap()
                .pop()
                .ok_or_else(|| BackendError::ProcessingFailed("No mock dimensions".to_string()))
        }

        fn read_metadata(&self, path: &Path) -> Result<ImageMetadata, BackendError> {
            self.operations
                .lock()
                .unwrap()
                .push(RecordedOp::ReadMetadata(path.to_string_lossy().to_string()));

            Ok(self
                .metadata_results
                .lock()
                .unwrap()
                .pop()
                .unwrap_or_default())
        }

        fn resize(&self, params: &ResizeParams) -> Result<(), BackendError> {
            self.operations.lock().unwrap().push(RecordedOp::Resize {
                source: params.source.to_string_lossy().to_string(),
                output: params.output.to_string_lossy().to_string(),
                width: params.width,
                height: params.height,
                quality: params.quality.value(),
            });
            Ok(())
        }

        fn thumbnail(&self, params: &ThumbnailParams) -> Result<(), BackendError> {
            self.operations.lock().unwrap().push(RecordedOp::Thumbnail {
                source: params.source.to_string_lossy().to_string(),
                output: params.output.to_string_lossy().to_string(),
                crop_width: params.crop_width,
                crop_height: params.crop_height,
                quality: params.quality.value(),
                sharpening: params.sharpening.map(|s| (s.sigma, s.threshold)),
            });
            Ok(())
        }
    }

    #[test]
    fn mock_records_identify() {
        let backend = MockBackend::with_dimensions(vec![Dimensions {
            width: 800,
            height: 600,
        }]);

        let result = backend.identify(Path::new("/test/image.jpg")).unwrap();
        assert_eq!(result.width, 800);
        assert_eq!(result.height, 600);

        let ops = backend.get_operations();
        assert_eq!(ops.len(), 1);
        assert!(matches!(&ops[0], RecordedOp::Identify(p) if p == "/test/image.jpg"));
    }

    #[test]
    fn mock_records_resize() {
        let backend = MockBackend::new();

        backend
            .resize(&ResizeParams {
                source: "/source.jpg".into(),
                output: "/output.avif".into(),
                width: 800,
                height: 600,
                quality: super::super::params::Quality::new(90),
            })
            .unwrap();

        let ops = backend.get_operations();
        assert_eq!(ops.len(), 1);
        assert!(matches!(
            &ops[0],
            RecordedOp::Resize {
                width: 800,
                height: 600,
                quality: 90,
                ..
            }
        ));
    }

    #[test]
    fn mock_records_thumbnail_with_sharpening() {
        let backend = MockBackend::new();

        backend
            .thumbnail(&ThumbnailParams {
                source: "/source.jpg".into(),
                output: "/thumb.webp".into(),
                crop_width: 400,
                crop_height: 500,
                quality: super::super::params::Quality::new(85),
                sharpening: Some(Sharpening::light()),
            })
            .unwrap();

        let ops = backend.get_operations();
        assert_eq!(ops.len(), 1);
        assert!(matches!(
            &ops[0],
            RecordedOp::Thumbnail {
                crop_width: 400,
                crop_height: 500,
                sharpening: Some((0.5, 0)),
                ..
            }
        ));
    }
}
