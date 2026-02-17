//! Pure Rust image processing backend — zero external dependencies.
//!
//! Everything is statically linked into the binary.
//!
//! ## Crate mapping
//!
//! | Operation | Crate / function |
//! |---|---|
//! | Decode (JPEG, PNG, TIFF, WebP) | `image` crate (pure Rust decoders) |
//! | Resize | `image::imageops::resize` with `Lanczos3` filter |
//! | Encode → AVIF | `image::codecs::avif::AvifEncoder` (rav1e, speed 6) |
//! | Thumbnail crop | `image::DynamicImage::resize_to_fill` |
//! | Sharpening | `image::imageops::unsharpen` |
//! | IPTC metadata | custom `iptc_parser` (JPEG APP13 + TIFF IFD) |

use super::backend::{BackendError, Dimensions, ImageBackend, ImageMetadata};
use super::params::{ResizeParams, ThumbnailParams};
use image::imageops::FilterType;
use image::{DynamicImage, ImageReader};
use std::path::Path;

/// Pure Rust backend using the `image` crate ecosystem.
///
/// See the [module docs](self) for the crate-to-operation mapping.
pub struct RustBackend;

impl RustBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RustBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Load and decode an image from disk.
fn load_image(path: &Path) -> Result<DynamicImage, BackendError> {
    ImageReader::open(path)
        .map_err(BackendError::Io)?
        .decode()
        .map_err(|e| {
            BackendError::ProcessingFailed(format!("Failed to decode {}: {}", path.display(), e))
        })
}

/// Save a DynamicImage to the given path, inferring format from extension.
fn save_image(img: &DynamicImage, path: &Path, quality: u32) -> Result<(), BackendError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "avif" => save_avif(img, path, quality),
        other => Err(BackendError::ProcessingFailed(format!(
            "Unsupported output format: {}",
            other
        ))),
    }
}

/// Encode and save as AVIF using ravif/rav1e (speed=6 for reasonable throughput).
fn save_avif(img: &DynamicImage, path: &Path, quality: u32) -> Result<(), BackendError> {
    let file = std::fs::File::create(path).map_err(BackendError::Io)?;
    let writer = std::io::BufWriter::new(file);
    let encoder =
        image::codecs::avif::AvifEncoder::new_with_speed_quality(writer, 6, quality as u8);
    img.write_with_encoder(encoder)
        .map_err(|e| BackendError::ProcessingFailed(format!("AVIF encode failed: {}", e)))
}

impl ImageBackend for RustBackend {
    fn identify(&self, path: &Path) -> Result<Dimensions, BackendError> {
        let (width, height) = image::image_dimensions(path).map_err(|e| {
            BackendError::ProcessingFailed(format!("Failed to read dimensions: {}", e))
        })?;
        Ok(Dimensions { width, height })
    }

    fn read_metadata(&self, path: &Path) -> Result<ImageMetadata, BackendError> {
        let iptc = super::iptc_parser::read_iptc(path);
        Ok(ImageMetadata {
            title: iptc.object_name,
            description: iptc.caption,
            keywords: iptc.keywords,
        })
    }

    fn resize(&self, params: &ResizeParams) -> Result<(), BackendError> {
        let img = load_image(&params.source)?;
        let resized = img.resize(params.width, params.height, FilterType::Lanczos3);
        save_image(&resized, &params.output, params.quality.value())
    }

    fn thumbnail(&self, params: &ThumbnailParams) -> Result<(), BackendError> {
        let img = load_image(&params.source)?;

        // Fill-resize then center-crop to exact dimensions
        let filled =
            img.resize_to_fill(params.crop_width, params.crop_height, FilterType::Lanczos3);

        // Apply sharpening if requested
        let final_img = if let Some(sharpening) = params.sharpening {
            DynamicImage::from(image::imageops::unsharpen(
                &filled,
                sharpening.sigma,
                sharpening.threshold,
            ))
        } else {
            filled
        };

        save_image(&final_img, &params.output, params.quality.value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imaging::params::{Quality, Sharpening};
    use image::{ImageEncoder, RgbImage};

    /// Create a small valid JPEG file with the given dimensions.
    fn create_test_jpeg(path: &Path, width: u32, height: u32) {
        let img = RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        let file = std::fs::File::create(path).unwrap();
        let writer = std::io::BufWriter::new(file);
        image::codecs::jpeg::JpegEncoder::new(writer)
            .write_image(img.as_raw(), width, height, image::ExtendedColorType::Rgb8)
            .unwrap();
    }

    #[test]
    fn identify_synthetic_jpeg() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("test.jpg");
        create_test_jpeg(&path, 200, 150);

        let backend = RustBackend::new();
        let dims = backend.identify(&path).unwrap();
        assert_eq!(dims.width, 200);
        assert_eq!(dims.height, 150);
    }

    #[test]
    fn identify_nonexistent_file_errors() {
        let backend = RustBackend::new();
        let result = backend.identify(Path::new("/nonexistent/image.jpg"));
        assert!(result.is_err());
    }

    #[test]
    fn read_metadata_synthetic_returns_default() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("test.jpg");
        create_test_jpeg(&path, 100, 100);

        let backend = RustBackend::new();
        let meta = backend.read_metadata(&path).unwrap();
        assert_eq!(meta, ImageMetadata::default());
    }

    #[test]
    fn read_metadata_nonexistent_returns_default() {
        let backend = RustBackend::new();
        let meta = backend
            .read_metadata(Path::new("/nonexistent/image.jpg"))
            .unwrap();
        assert_eq!(meta, ImageMetadata::default());
    }

    #[test]
    fn resize_synthetic_to_avif() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("source.jpg");
        create_test_jpeg(&source, 400, 300);

        let output = tmp.path().join("resized.avif");
        let backend = RustBackend::new();
        backend
            .resize(&ResizeParams {
                source,
                output: output.clone(),
                width: 200,
                height: 150,
                quality: Quality::new(85),
            })
            .unwrap();

        assert!(output.exists());
        assert!(std::fs::metadata(&output).unwrap().len() > 0);
    }

    #[test]
    fn resize_unsupported_format_errors() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("source.jpg");
        create_test_jpeg(&source, 100, 100);

        let output = tmp.path().join("output.webp");
        let backend = RustBackend::new();
        let result = backend.resize(&ResizeParams {
            source,
            output,
            width: 50,
            height: 50,
            quality: Quality::new(85),
        });
        assert!(result.is_err());
    }

    #[test]
    fn thumbnail_synthetic_exact_dimensions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("source.jpg");
        create_test_jpeg(&source, 800, 600);

        let output = tmp.path().join("thumb.avif");
        let backend = RustBackend::new();
        backend
            .thumbnail(&ThumbnailParams {
                source,
                output: output.clone(),
                crop_width: 400,
                crop_height: 500,
                quality: Quality::new(85),
                sharpening: Some(Sharpening::light()),
            })
            .unwrap();

        assert!(output.exists());
        assert!(std::fs::metadata(&output).unwrap().len() > 0);
    }

    #[test]
    fn thumbnail_synthetic_portrait_source() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("source.jpg");
        create_test_jpeg(&source, 600, 800);

        let output = tmp.path().join("thumb.avif");
        let backend = RustBackend::new();
        backend
            .thumbnail(&ThumbnailParams {
                source,
                output: output.clone(),
                crop_width: 400,
                crop_height: 500,
                quality: Quality::new(85),
                sharpening: Some(Sharpening::light()),
            })
            .unwrap();

        assert!(output.exists());
        assert!(std::fs::metadata(&output).unwrap().len() > 0);
    }

    #[test]
    fn thumbnail_synthetic_without_sharpening() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("source.jpg");
        create_test_jpeg(&source, 400, 300);

        let output = tmp.path().join("thumb.avif");
        let backend = RustBackend::new();
        backend
            .thumbnail(&ThumbnailParams {
                source,
                output: output.clone(),
                crop_width: 200,
                crop_height: 200,
                quality: Quality::new(85),
                sharpening: None,
            })
            .unwrap();

        assert!(output.exists());
        assert!(std::fs::metadata(&output).unwrap().len() > 0);
    }
}
