//! Image processing backend trait and implementations.
//!
//! The `ImageBackend` trait abstracts the actual image processing,
//! allowing for different implementations (ImageMagick, pure Rust, mock).

use super::params::{ResizeParams, ThumbnailParams};
use std::path::Path;
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BackendError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Command failed: {0}")]
    CommandFailed(String),
    #[error("Invalid output: {0}")]
    InvalidOutput(String),
}

/// Result of an identify operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dimensions {
    pub width: u32,
    pub height: u32,
}

/// Trait for image processing backends.
///
/// Implementations execute the actual image operations.
/// This allows for:
/// - Different backends (ImageMagick, pure Rust)
/// - Mock backends for testing
pub trait ImageBackend: Sync {
    /// Get image dimensions.
    fn identify(&self, path: &Path) -> Result<Dimensions, BackendError>;

    /// Execute a resize operation.
    fn resize(&self, params: &ResizeParams) -> Result<(), BackendError>;

    /// Execute a thumbnail operation (resize + center crop).
    fn thumbnail(&self, params: &ThumbnailParams) -> Result<(), BackendError>;
}

/// ImageMagick backend using the `convert` command.
///
/// Uses ImageMagick 6's `convert` and `identify` commands.
/// This is the standard on Ubuntu/Debian and CI environments.
pub struct ImageMagickBackend;

impl ImageMagickBackend {
    pub fn new() -> Self {
        Self
    }

    fn run_convert(&self, args: &[&str]) -> Result<(), BackendError> {
        let output = Command::new("convert").args(args).output()?;

        if !output.status.success() {
            return Err(BackendError::CommandFailed(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        Ok(())
    }
}

impl Default for ImageMagickBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageBackend for ImageMagickBackend {
    fn identify(&self, path: &Path) -> Result<Dimensions, BackendError> {
        let output = Command::new("identify")
            .args(["-format", "%w %h", path.to_str().unwrap()])
            .output()?;

        if !output.status.success() {
            return Err(BackendError::CommandFailed(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let dims = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = dims.split_whitespace().collect();

        if parts.len() != 2 {
            return Err(BackendError::InvalidOutput(format!(
                "Expected 'width height', got: {}",
                dims
            )));
        }

        let width: u32 = parts[0]
            .parse()
            .map_err(|_| BackendError::InvalidOutput(format!("Invalid width: {}", parts[0])))?;
        let height: u32 = parts[1]
            .parse()
            .map_err(|_| BackendError::InvalidOutput(format!("Invalid height: {}", parts[1])))?;

        Ok(Dimensions { width, height })
    }

    fn resize(&self, params: &ResizeParams) -> Result<(), BackendError> {
        let size = format!("{}x{}", params.width, params.height);
        let quality = params.quality.value().to_string();

        // Determine output format and add format-specific options
        let output_path = params.output.to_str().unwrap();
        let is_avif = output_path.ends_with(".avif");

        let mut args = vec![
            params.source.to_str().unwrap(),
            "-resize",
            &size,
            "-quality",
            &quality,
        ];

        // AVIF-specific: speed setting for faster encoding
        let heic_speed;
        if is_avif {
            heic_speed = "heic:speed=6".to_string();
            args.push("-define");
            args.push(&heic_speed);
        }

        args.push(output_path);

        self.run_convert(&args)
    }

    fn thumbnail(&self, params: &ThumbnailParams) -> Result<(), BackendError> {
        let fill_size = format!("{}x{}^", params.fill_width, params.fill_height);
        let crop_size = format!("{}x{}", params.crop_width, params.crop_height);
        let quality = params.quality.value().to_string();

        let mut args = vec![
            params.source.to_str().unwrap(),
            "-resize",
            &fill_size,
            "-gravity",
            "center",
            "-extent",
            &crop_size,
            "-quality",
            &quality,
        ];

        // Optional sharpening
        let sharpen_str;
        if let Some(sharpening) = params.sharpening {
            sharpen_str = format!("{}x{}", sharpening.radius, sharpening.sigma);
            args.push("-sharpen");
            args.push(&sharpen_str);
        }

        args.push(params.output.to_str().unwrap());

        self.run_convert(&args)
    }
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
        pub operations: Mutex<Vec<RecordedOp>>,
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum RecordedOp {
        Identify(String),
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
            fill_width: u32,
            fill_height: u32,
            crop_width: u32,
            crop_height: u32,
            quality: u32,
            sharpening: Option<(f32, f32)>,
        },
    }

    impl MockBackend {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn with_dimensions(dims: Vec<Dimensions>) -> Self {
            Self {
                identify_results: Mutex::new(dims),
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
                .ok_or_else(|| BackendError::InvalidOutput("No mock dimensions".to_string()))
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
                fill_width: params.fill_width,
                fill_height: params.fill_height,
                crop_width: params.crop_width,
                crop_height: params.crop_height,
                quality: params.quality.value(),
                sharpening: params.sharpening.map(|s| (s.radius, s.sigma)),
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
                fill_width: 500,
                fill_height: 625,
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
                sharpening: Some((0.0, 0.5)),
                ..
            }
        ));
    }
}
