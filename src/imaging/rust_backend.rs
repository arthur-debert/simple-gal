//! Pure Rust image processing backend using the `image` crate.
//!
//! Replaces ImageMagick shell-outs with compiled-in Rust equivalents.
//! Zero runtime dependencies — everything is statically linked.

use super::backend::{BackendError, Dimensions, ImageBackend, ImageMetadata};
use super::params::{ResizeParams, ThumbnailParams};
use image::imageops::FilterType;
use image::{DynamicImage, ImageReader};
use std::path::Path;

/// Pure Rust backend using the `image` crate ecosystem.
///
/// - Decoding: JPEG, PNG, WebP (pure Rust)
/// - Encoding: WebP lossy (vendored libwebp via `webp` crate), AVIF (rav1e)
/// - Operations: Lanczos3 resize, center-crop, unsharpen
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
        "webp" => save_webp(img, path, quality),
        "avif" => save_avif(img, path, quality),
        other => Err(BackendError::ProcessingFailed(format!(
            "Unsupported output format: {}",
            other
        ))),
    }
}

/// Encode and save as lossy WebP using the `webp` crate (vendored libwebp).
fn save_webp(img: &DynamicImage, path: &Path, quality: u32) -> Result<(), BackendError> {
    let encoder = webp::Encoder::from_image(img)
        .map_err(|e| BackendError::ProcessingFailed(format!("WebP encoder init failed: {}", e)))?;
    let encoded = encoder.encode(quality as f32);
    std::fs::write(path, &*encoded).map_err(BackendError::Io)
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
    use std::path::PathBuf;

    #[test]
    fn identify_reads_jpeg_dimensions() {
        let backend = RustBackend::new();
        // Use a real test image from the content directory
        let path = PathBuf::from("content/001-NY/Q1020899.jpg");
        if !path.exists() {
            return; // Skip if content not available
        }
        let dims = backend.identify(&path).unwrap();
        assert!(dims.width > 0);
        assert!(dims.height > 0);
    }

    #[test]
    fn identify_nonexistent_file_errors() {
        let backend = RustBackend::new();
        let result = backend.identify(Path::new("/nonexistent/image.jpg"));
        assert!(result.is_err());
    }

    #[test]
    fn read_metadata_returns_default_for_non_jpeg() {
        let backend = RustBackend::new();
        // Non-existent file should return default metadata (not error)
        let result = backend.read_metadata(Path::new("/nonexistent/image.jpg"));
        assert!(result.is_ok());
        let meta = result.unwrap();
        assert_eq!(meta, ImageMetadata::default());
    }

    #[test]
    fn resize_produces_webp_output() {
        let source = PathBuf::from("content/001-NY/Q1020899.jpg");
        if !source.exists() {
            return;
        }
        let backend = RustBackend::new();
        let output = PathBuf::from("/tmp/simple-gal-test-resize.webp");
        backend
            .resize(&ResizeParams {
                source,
                output: output.clone(),
                width: 800,
                height: 600,
                quality: Quality::new(85),
            })
            .unwrap();
        assert!(output.exists());
        let dims = backend.identify(&output).unwrap();
        // resize() preserves aspect ratio, so at least one dimension should be <= target
        assert!(dims.width <= 800 && dims.height <= 600);
        std::fs::remove_file(&output).ok();
    }

    #[test]
    fn resize_produces_avif_output() {
        let source = PathBuf::from("content/001-NY/Q1020899.jpg");
        if !source.exists() {
            return;
        }
        let backend = RustBackend::new();
        let output = PathBuf::from("/tmp/simple-gal-test-resize.avif");
        backend
            .resize(&ResizeParams {
                source,
                output: output.clone(),
                width: 800,
                height: 600,
                quality: Quality::new(85),
            })
            .unwrap();
        assert!(output.exists());
        assert!(std::fs::metadata(&output).unwrap().len() > 0);
        std::fs::remove_file(&output).ok();
    }

    #[test]
    fn thumbnail_produces_cropped_webp() {
        let source = PathBuf::from("content/001-NY/Q1020899.jpg");
        if !source.exists() {
            return;
        }
        let backend = RustBackend::new();
        let output = PathBuf::from("/tmp/simple-gal-test-thumb.webp");
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
        let dims = backend.identify(&output).unwrap();
        assert_eq!(dims.width, 400);
        assert_eq!(dims.height, 500);
        std::fs::remove_file(&output).ok();
    }
}
