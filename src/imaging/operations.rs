//! High-level image operations.
//!
//! These functions combine calculations with backend execution.
//! They take configuration, compute parameters, and call the backend.

use super::backend::{BackendError, ImageBackend};
use super::calculations::{
    ResponsiveSize, calculate_responsive_sizes, calculate_thumbnail_dimensions,
};
use super::params::{Quality, ResizeParams, Sharpening, ThumbnailParams};
use std::path::Path;

/// Result type for image operations.
pub type Result<T> = std::result::Result<T, BackendError>;

/// Get image dimensions using the backend.
pub fn get_dimensions(backend: &impl ImageBackend, path: &Path) -> Result<(u32, u32)> {
    let dims = backend.identify(path)?;
    Ok((dims.width, dims.height))
}

/// Generated image variant with paths and dimensions.
#[derive(Debug, Clone)]
pub struct GeneratedVariant {
    pub target_size: u32,
    pub avif_path: String,
    pub webp_path: String,
    pub width: u32,
    pub height: u32,
}

/// Configuration for responsive image generation.
#[derive(Debug, Clone)]
pub struct ResponsiveConfig {
    pub sizes: Vec<u32>,
    pub quality: Quality,
}

/// Create responsive images at multiple sizes.
///
/// Generates AVIF and WebP variants for each applicable size.
/// Sizes larger than the original are skipped.
pub fn create_responsive_images(
    backend: &impl ImageBackend,
    source: &Path,
    output_dir: &Path,
    filename_stem: &str,
    original_dims: (u32, u32),
    config: &ResponsiveConfig,
) -> Result<Vec<GeneratedVariant>> {
    let sizes = calculate_responsive_sizes(original_dims, &config.sizes);
    let mut variants = Vec::new();

    for ResponsiveSize {
        target,
        width,
        height,
    } in sizes
    {
        let avif_name = format!("{}-{}.avif", filename_stem, target);
        let webp_name = format!("{}-{}.webp", filename_stem, target);
        let avif_path = output_dir.join(&avif_name);
        let webp_path = output_dir.join(&webp_name);

        // Generate AVIF
        backend.resize(&ResizeParams {
            source: source.to_path_buf(),
            output: avif_path.clone(),
            width,
            height,
            quality: config.quality,
        })?;

        // Generate WebP
        backend.resize(&ResizeParams {
            source: source.to_path_buf(),
            output: webp_path.clone(),
            width,
            height,
            quality: config.quality,
        })?;

        // Compute relative path for manifest
        let relative_dir = output_dir
            .file_name()
            .map(|s| s.to_str().unwrap())
            .unwrap_or("");

        variants.push(GeneratedVariant {
            target_size: target,
            avif_path: format!("{}/{}", relative_dir, avif_name),
            webp_path: format!("{}/{}", relative_dir, webp_name),
            width,
            height,
        });
    }

    Ok(variants)
}

/// Configuration for thumbnail generation.
#[derive(Debug, Clone)]
pub struct ThumbnailConfig {
    pub aspect: (u32, u32),
    pub short_edge: u32,
    pub quality: Quality,
    pub sharpening: Option<Sharpening>,
}

impl Default for ThumbnailConfig {
    fn default() -> Self {
        Self {
            aspect: (4, 5),
            short_edge: 400,
            quality: Quality::default(),
            sharpening: Some(Sharpening::light()),
        }
    }
}

/// Plan a thumbnail operation without executing it.
///
/// Useful for testing parameter generation.
pub fn plan_thumbnail(
    source: &Path,
    output_path: &Path,
    config: &ThumbnailConfig,
) -> ThumbnailParams {
    let (crop_w, crop_h) = calculate_thumbnail_dimensions(config.aspect, config.short_edge);

    ThumbnailParams {
        source: source.to_path_buf(),
        output: output_path.to_path_buf(),
        crop_width: crop_w,
        crop_height: crop_h,
        quality: config.quality,
        sharpening: config.sharpening,
    }
}

/// Create a thumbnail image.
///
/// Resizes to fill the target aspect ratio, then center-crops.
pub fn create_thumbnail(
    backend: &impl ImageBackend,
    source: &Path,
    output_dir: &Path,
    filename_stem: &str,
    config: &ThumbnailConfig,
) -> Result<String> {
    let thumb_name = format!("{}-thumb.webp", filename_stem);
    let thumb_path = output_dir.join(&thumb_name);

    let params = plan_thumbnail(source, &thumb_path, config);
    backend.thumbnail(&params)?;

    let relative_dir = output_dir
        .file_name()
        .map(|s| s.to_str().unwrap())
        .unwrap_or("");

    Ok(format!("{}/{}", relative_dir, thumb_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imaging::Dimensions;
    use crate::imaging::backend::tests::{MockBackend, RecordedOp};

    #[test]
    fn get_dimensions_calls_backend() {
        let backend = MockBackend::with_dimensions(vec![Dimensions {
            width: 1920,
            height: 1080,
        }]);

        let dims = get_dimensions(&backend, Path::new("/test.jpg")).unwrap();
        assert_eq!(dims, (1920, 1080));
    }

    #[test]
    fn plan_thumbnail_calculates_crop_dimensions() {
        // 4:5 portrait thumb at 400 short edge → crop 400x500
        let params = plan_thumbnail(
            Path::new("/source.jpg"),
            Path::new("/thumb.webp"),
            &ThumbnailConfig::default(),
        );

        assert_eq!(params.crop_width, 400);
        assert_eq!(params.crop_height, 500);
    }

    #[test]
    fn plan_thumbnail_landscape_aspect() {
        let config = ThumbnailConfig {
            aspect: (16, 9),
            short_edge: 180,
            ..ThumbnailConfig::default()
        };
        let params = plan_thumbnail(Path::new("/source.jpg"), Path::new("/thumb.webp"), &config);

        assert_eq!(params.crop_width, 320);
        assert_eq!(params.crop_height, 180);
    }

    #[test]
    fn create_thumbnail_uses_backend() {
        let backend = MockBackend::new();
        let config = ThumbnailConfig::default();

        let result = create_thumbnail(
            &backend,
            Path::new("/source.jpg"),
            Path::new("/output"),
            "001-test",
            &config,
        )
        .unwrap();

        assert_eq!(result, "output/001-test-thumb.webp");

        let ops = backend.get_operations();
        assert_eq!(ops.len(), 1);
        assert!(matches!(
            &ops[0],
            RecordedOp::Thumbnail {
                crop_width: 400,
                crop_height: 500,
                ..
            }
        ));
    }

    #[test]
    fn create_responsive_skips_larger_sizes() {
        let backend = MockBackend::new();
        let config = ResponsiveConfig {
            sizes: vec![800, 1400, 2080],
            quality: Quality::default(),
        };

        // Original is 1000px - should skip 1400 and 2080
        let variants = create_responsive_images(
            &backend,
            Path::new("/source.jpg"),
            Path::new("/output"),
            "001-test",
            (1000, 750),
            &config,
        )
        .unwrap();

        assert_eq!(variants.len(), 1);
        assert_eq!(variants[0].target_size, 800);

        // Should have 2 operations (AVIF + WebP)
        let ops = backend.get_operations();
        assert_eq!(ops.len(), 2);
    }

    #[test]
    fn create_responsive_generates_all_formats() {
        let backend = MockBackend::new();
        let config = ResponsiveConfig {
            sizes: vec![800],
            quality: Quality::new(85),
        };

        create_responsive_images(
            &backend,
            Path::new("/source.jpg"),
            Path::new("/output"),
            "001-test",
            (2000, 1500),
            &config,
        )
        .unwrap();

        let ops = backend.get_operations();
        assert_eq!(ops.len(), 2);

        // First should be AVIF
        assert!(matches!(
            &ops[0],
            RecordedOp::Resize { output, quality: 85, .. } if output.ends_with(".avif")
        ));

        // Second should be WebP
        assert!(matches!(
            &ops[1],
            RecordedOp::Resize { output, quality: 85, .. } if output.ends_with(".webp")
        ));
    }

    #[test]
    fn create_responsive_fallback_to_original_size() {
        let backend = MockBackend::new();
        let config = ResponsiveConfig {
            sizes: vec![800, 1400],
            quality: Quality::default(),
        };

        // Original is only 500px - smaller than all targets
        let variants = create_responsive_images(
            &backend,
            Path::new("/source.jpg"),
            Path::new("/output"),
            "001-test",
            (500, 400),
            &config,
        )
        .unwrap();

        assert_eq!(variants.len(), 1);
        assert_eq!(variants[0].target_size, 500); // Uses original size
        assert_eq!(variants[0].width, 500);
        assert_eq!(variants[0].height, 400);
    }
}
