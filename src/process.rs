//! Image processing and responsive image generation.
//!
//! Stage 2 of the LightTable build pipeline. Takes the manifest from the scan stage
//! and processes all images to generate responsive sizes and thumbnails.
//!
//! ## Dependencies
//!
//! Requires ImageMagick to be installed. Uses the `convert` and `identify` commands.
//!
//! ## Output Formats
//!
//! For each source image, generates:
//! - **Responsive images**: Multiple sizes in AVIF and WebP formats
//! - **Thumbnails**: Fixed aspect ratio crops for gallery grids
//!
//! ## Default Configuration
//!
//! ```text
//! Responsive sizes: 800px, 1400px, 2080px (on the longer edge)
//! Quality: 90%
//! Thumbnail aspect: 4:5 (portrait)
//! Thumbnail size: 400px (on the short edge)
//! ```
//!
//! ## Output Structure
//!
//! ```text
//! processed/
//! ├── manifest.json              # Updated manifest with generated paths
//! ├── 010-Landscapes/
//! │   ├── 001-dawn-800.avif      # Responsive sizes
//! │   ├── 001-dawn-800.webp
//! │   ├── 001-dawn-1400.avif
//! │   ├── 001-dawn-1400.webp
//! │   ├── 001-dawn-2080.avif
//! │   ├── 001-dawn-2080.webp
//! │   └── 001-dawn-thumb.webp    # 4:5 center-cropped thumbnail
//! └── ...
//! ```
//!
//! ## Parallel Processing
//!
//! Images are processed in parallel using [rayon](https://docs.rs/rayon) for
//! optimal performance on multi-core systems.

use crate::config::SiteConfig;
use crate::imaging::{
    BackendError, ImageBackend, ImageMagickBackend, Quality, ResponsiveConfig, Sharpening,
    ThumbnailConfig, create_responsive_images, create_thumbnail, get_dimensions,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProcessError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Image processing failed: {0}")]
    Imaging(#[from] BackendError),
    #[error("Source image not found: {0}")]
    SourceNotFound(PathBuf),
}

/// Configuration for image processing
#[derive(Debug, Clone)]
pub struct ProcessConfig {
    pub sizes: Vec<u32>,
    pub quality: u32,
    pub thumbnail_aspect: (u32, u32), // width, height
    pub thumbnail_size: u32,          // size on the short edge
}

impl ProcessConfig {
    /// Build a ProcessConfig from SiteConfig values.
    pub fn from_site_config(config: &SiteConfig) -> Self {
        let ar = config.thumbnails.aspect_ratio;
        Self {
            sizes: config.images.sizes.clone(),
            quality: config.images.quality,
            thumbnail_aspect: (ar[0], ar[1]),
            thumbnail_size: 400,
        }
    }
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self::from_site_config(&SiteConfig::default())
    }
}

/// About page content
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AboutPage {
    /// Title from markdown content (first # heading)
    pub title: String,
    /// Link title from filename (dashes to spaces)
    pub link_title: String,
    /// Raw markdown body content
    pub body: String,
}

/// Input manifest (from scan stage)
#[derive(Debug, Deserialize)]
pub struct InputManifest {
    pub navigation: Vec<NavItem>,
    pub albums: Vec<InputAlbum>,
    pub about: Option<AboutPage>,
    pub config: SiteConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NavItem {
    pub title: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<NavItem>,
}

#[derive(Debug, Deserialize)]
pub struct InputAlbum {
    pub path: String,
    pub title: String,
    pub description: Option<String>,
    pub preview_image: String,
    pub images: Vec<InputImage>,
    pub in_nav: bool,
}

#[derive(Debug, Deserialize)]
pub struct InputImage {
    pub number: u32,
    pub source_path: String,
    pub filename: String,
}

/// Output manifest (after processing)
#[derive(Debug, Serialize)]
pub struct OutputManifest {
    pub navigation: Vec<NavItem>,
    pub albums: Vec<OutputAlbum>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub about: Option<AboutPage>,
    pub config: SiteConfig,
}

#[derive(Debug, Serialize)]
pub struct OutputAlbum {
    pub path: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub preview_image: String,
    pub thumbnail: String,
    pub images: Vec<OutputImage>,
    pub in_nav: bool,
}

#[derive(Debug, Serialize)]
pub struct OutputImage {
    pub number: u32,
    pub source_path: String,
    /// Original dimensions (width, height)
    pub dimensions: (u32, u32),
    /// Generated responsive images: { "800": { "avif": "path", "webp": "path" }, ... }
    pub generated: std::collections::BTreeMap<String, GeneratedVariant>,
    /// Thumbnail path
    pub thumbnail: String,
}

#[derive(Debug, Serialize)]
pub struct GeneratedVariant {
    pub avif: String,
    pub webp: String,
    pub width: u32,
    pub height: u32,
}

pub fn process(
    manifest_path: &Path,
    source_root: &Path,
    output_dir: &Path,
    config: &ProcessConfig,
) -> Result<OutputManifest, ProcessError> {
    let backend = ImageMagickBackend::new();
    process_with_backend(&backend, manifest_path, source_root, output_dir, config)
}

/// Process images using a specific backend (allows testing with mock).
pub fn process_with_backend(
    backend: &impl ImageBackend,
    manifest_path: &Path,
    source_root: &Path,
    output_dir: &Path,
    config: &ProcessConfig,
) -> Result<OutputManifest, ProcessError> {
    let manifest_content = std::fs::read_to_string(manifest_path)?;
    let input: InputManifest = serde_json::from_str(&manifest_content)?;

    std::fs::create_dir_all(output_dir)?;

    let responsive_config = ResponsiveConfig {
        sizes: config.sizes.clone(),
        quality: Quality::new(config.quality),
    };

    let thumbnail_config = ThumbnailConfig {
        aspect: config.thumbnail_aspect,
        short_edge: config.thumbnail_size,
        quality: Quality::new(config.quality),
        sharpening: Some(Sharpening::light()),
    };

    let mut output_albums = Vec::new();

    for album in &input.albums {
        println!("Processing album: {}", album.title);
        let album_output_dir = output_dir.join(&album.path);
        std::fs::create_dir_all(&album_output_dir)?;

        // Process images (sequentially when using backend reference)
        let mut processed_images = Vec::new();

        for image in &album.images {
            let source_path = source_root.join(&image.source_path);
            if !source_path.exists() {
                return Err(ProcessError::SourceNotFound(source_path));
            }

            println!("  {} ", image.filename);

            // Get original dimensions
            let dimensions = get_dimensions(backend, &source_path)?;

            // Get filename stem
            let stem = Path::new(&image.filename)
                .file_stem()
                .unwrap()
                .to_str()
                .unwrap();

            // Generate responsive sizes
            let variants = create_responsive_images(
                backend,
                &source_path,
                &album_output_dir,
                stem,
                dimensions,
                &responsive_config,
            )?;

            // Generate thumbnail
            let thumbnail_path = create_thumbnail(
                backend,
                &source_path,
                &album_output_dir,
                stem,
                dimensions,
                &thumbnail_config,
            )?;

            // Convert variants to BTreeMap
            let generated: std::collections::BTreeMap<String, GeneratedVariant> = variants
                .into_iter()
                .map(|v| {
                    (
                        v.target_size.to_string(),
                        GeneratedVariant {
                            avif: v.avif_path,
                            webp: v.webp_path,
                            width: v.width,
                            height: v.height,
                        },
                    )
                })
                .collect();

            processed_images.push((image, dimensions, generated, thumbnail_path));
        }

        // Build output images (preserving order)
        let mut output_images: Vec<OutputImage> = processed_images
            .into_iter()
            .map(
                |(image, dimensions, generated, thumbnail_path)| OutputImage {
                    number: image.number,
                    source_path: image.source_path.clone(),
                    dimensions,
                    generated,
                    thumbnail: thumbnail_path,
                },
            )
            .collect();

        // Sort by number to ensure consistent ordering
        output_images.sort_by_key(|img| img.number);

        // Find album thumbnail (first image or image #1)
        let album_thumbnail = output_images
            .iter()
            .find(|img| img.number == 1)
            .or_else(|| output_images.first())
            .map(|img| img.thumbnail.clone())
            .unwrap_or_default();

        output_albums.push(OutputAlbum {
            path: album.path.clone(),
            title: album.title.clone(),
            description: album.description.clone(),
            preview_image: album.preview_image.clone(),
            thumbnail: album_thumbnail,
            images: output_images,
            in_nav: album.in_nav,
        });
    }

    Ok(OutputManifest {
        navigation: input.navigation,
        albums: output_albums,
        about: input.about,
        config: input.config,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // =========================================================================
    // ProcessConfig tests (no ImageMagick required)
    // =========================================================================

    #[test]
    fn process_config_default_values() {
        let config = ProcessConfig::default();

        assert_eq!(config.sizes, vec![800, 1400, 2080]);
        assert_eq!(config.quality, 90);
        assert_eq!(config.thumbnail_aspect, (4, 5));
        assert_eq!(config.thumbnail_size, 400);
    }

    #[test]
    fn process_config_custom_values() {
        let config = ProcessConfig {
            sizes: vec![100, 200],
            quality: 85,
            thumbnail_aspect: (1, 1),
            thumbnail_size: 150,
        };

        assert_eq!(config.sizes, vec![100, 200]);
        assert_eq!(config.quality, 85);
        assert_eq!(config.thumbnail_aspect, (1, 1));
        assert_eq!(config.thumbnail_size, 150);
    }

    // =========================================================================
    // Manifest parsing tests (no ImageMagick required)
    // =========================================================================

    #[test]
    fn parse_input_manifest() {
        let manifest_json = r##"{
            "navigation": [
                {"title": "Album", "path": "010-album", "children": []}
            ],
            "albums": [{
                "path": "010-album",
                "title": "Album",
                "description": "A test album",
                "preview_image": "010-album/001-test.jpg",
                "images": [{
                    "number": 1,
                    "source_path": "010-album/001-test.jpg",
                    "filename": "001-test.jpg"
                }],
                "in_nav": true
            }],
            "about": {
                "title": "About",
                "link_title": "about",
                "body": "# About\n\nContent"
            },
            "config": {
                "colors": {
                    "light": {
                        "background": "#fff",
                        "text": "#000",
                        "text_muted": "#666",
                        "border": "#ccc",
                        "link": "#00f",
                        "link_hover": "#f00"
                    },
                    "dark": {
                        "background": "#000",
                        "text": "#fff",
                        "text_muted": "#999",
                        "border": "#333",
                        "link": "#88f",
                        "link_hover": "#f88"
                    }
                }
            }
        }"##;

        let manifest: InputManifest = serde_json::from_str(manifest_json).unwrap();

        assert_eq!(manifest.navigation.len(), 1);
        assert_eq!(manifest.navigation[0].title, "Album");
        assert_eq!(manifest.albums.len(), 1);
        assert_eq!(manifest.albums[0].title, "Album");
        assert_eq!(
            manifest.albums[0].description,
            Some("A test album".to_string())
        );
        assert_eq!(manifest.albums[0].images.len(), 1);
        assert!(manifest.about.is_some());
        assert_eq!(manifest.about.as_ref().unwrap().title, "About");
    }

    #[test]
    fn parse_manifest_without_about() {
        let manifest_json = r##"{
            "navigation": [],
            "albums": [],
            "config": {
                "colors": {
                    "light": {
                        "background": "#fff",
                        "text": "#000",
                        "text_muted": "#666",
                        "border": "#ccc",
                        "link": "#00f",
                        "link_hover": "#f00"
                    },
                    "dark": {
                        "background": "#000",
                        "text": "#fff",
                        "text_muted": "#999",
                        "border": "#333",
                        "link": "#88f",
                        "link_hover": "#f88"
                    }
                }
            }
        }"##;

        let manifest: InputManifest = serde_json::from_str(manifest_json).unwrap();
        assert!(manifest.about.is_none());
    }

    #[test]
    fn parse_nav_item_with_children() {
        let json = r#"{
            "title": "Travel",
            "path": "020-travel",
            "children": [
                {"title": "Japan", "path": "020-travel/010-japan", "children": []},
                {"title": "Italy", "path": "020-travel/020-italy", "children": []}
            ]
        }"#;

        let item: NavItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.title, "Travel");
        assert_eq!(item.children.len(), 2);
        assert_eq!(item.children[0].title, "Japan");
    }

    // =========================================================================
    // Process with mock backend tests (no ImageMagick required)
    // =========================================================================

    use crate::imaging::Dimensions;
    use crate::imaging::backend::tests::MockBackend;

    fn create_test_manifest(tmp: &Path) -> PathBuf {
        let manifest = r##"{
            "navigation": [],
            "albums": [{
                "path": "test-album",
                "title": "Test Album",
                "description": null,
                "preview_image": "test-album/001-test.jpg",
                "images": [{
                    "number": 1,
                    "source_path": "test-album/001-test.jpg",
                    "filename": "001-test.jpg"
                }],
                "in_nav": true
            }],
            "config": {
                "colors": {
                    "light": {
                        "background": "#fff",
                        "text": "#000",
                        "text_muted": "#666",
                        "border": "#ccc",
                        "link": "#00f",
                        "link_hover": "#f00"
                    },
                    "dark": {
                        "background": "#000",
                        "text": "#fff",
                        "text_muted": "#999",
                        "border": "#333",
                        "link": "#88f",
                        "link_hover": "#f88"
                    }
                }
            }
        }"##;

        let manifest_path = tmp.join("manifest.json");
        fs::write(&manifest_path, manifest).unwrap();
        manifest_path
    }

    fn create_dummy_source(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Just create an empty file - the mock backend doesn't need real content
        fs::write(path, "").unwrap();
    }

    #[test]
    fn process_with_mock_generates_correct_outputs() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let output_dir = tmp.path().join("output");

        // Create dummy source file
        let image_path = source_dir.join("test-album/001-test.jpg");
        create_dummy_source(&image_path);

        // Create manifest
        let manifest_path = create_test_manifest(tmp.path());

        // Create mock backend with dimensions
        let backend = MockBackend::with_dimensions(vec![Dimensions {
            width: 200,
            height: 250,
        }]);

        // Process
        let config = ProcessConfig {
            sizes: vec![100, 150],
            thumbnail_size: 80,
            ..Default::default()
        };

        let result =
            process_with_backend(&backend, &manifest_path, &source_dir, &output_dir, &config)
                .unwrap();

        // Verify outputs
        assert_eq!(result.albums.len(), 1);
        assert_eq!(result.albums[0].images.len(), 1);

        let image = &result.albums[0].images[0];
        assert_eq!(image.dimensions, (200, 250));
        assert!(!image.generated.is_empty());
        assert!(!image.thumbnail.is_empty());
    }

    #[test]
    fn process_with_mock_records_correct_operations() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let output_dir = tmp.path().join("output");

        let image_path = source_dir.join("test-album/001-test.jpg");
        create_dummy_source(&image_path);

        let manifest_path = create_test_manifest(tmp.path());

        // 2000x1500 landscape - should generate both sizes
        let backend = MockBackend::with_dimensions(vec![Dimensions {
            width: 2000,
            height: 1500,
        }]);

        let config = ProcessConfig {
            sizes: vec![800, 1400],
            quality: 85,
            thumbnail_size: 100,
            ..Default::default()
        };

        process_with_backend(&backend, &manifest_path, &source_dir, &output_dir, &config).unwrap();

        use crate::imaging::backend::tests::RecordedOp;
        let ops = backend.get_operations();

        // Should have: 1 identify + 4 resizes (2 sizes × 2 formats) + 1 thumbnail = 6 ops
        assert_eq!(ops.len(), 6);

        // First is identify
        assert!(matches!(&ops[0], RecordedOp::Identify(_)));

        // Then resizes with correct quality
        for op in &ops[1..5] {
            assert!(matches!(op, RecordedOp::Resize { quality: 85, .. }));
        }

        // Last is thumbnail
        assert!(matches!(&ops[5], RecordedOp::Thumbnail { .. }));
    }

    #[test]
    fn process_with_mock_skips_larger_sizes() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let output_dir = tmp.path().join("output");

        let image_path = source_dir.join("test-album/001-test.jpg");
        create_dummy_source(&image_path);

        let manifest_path = create_test_manifest(tmp.path());

        // 500x400 - smaller than all requested sizes
        let backend = MockBackend::with_dimensions(vec![Dimensions {
            width: 500,
            height: 400,
        }]);

        let config = ProcessConfig {
            sizes: vec![800, 1400, 2080],
            ..Default::default()
        };

        let result =
            process_with_backend(&backend, &manifest_path, &source_dir, &output_dir, &config)
                .unwrap();

        // Should only have original size
        let image = &result.albums[0].images[0];
        assert_eq!(image.generated.len(), 1);
        assert!(image.generated.contains_key("500"));
    }

    #[test]
    fn process_source_not_found_error() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let output_dir = tmp.path().join("output");

        // Don't create the source file
        let manifest_path = create_test_manifest(tmp.path());
        let backend = MockBackend::new();
        let config = ProcessConfig::default();

        let result =
            process_with_backend(&backend, &manifest_path, &source_dir, &output_dir, &config);

        assert!(matches!(result, Err(ProcessError::SourceNotFound(_))));
    }

    // =========================================================================
    // ImageMagick integration tests (require ImageMagick)
    // =========================================================================

    fn create_test_image(path: &Path) {
        // Create a 200x250 test image (4:5 aspect)
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::process::Command::new("convert")
            .args([
                "-size",
                "200x250",
                "xc:gray",
                "-fill",
                "white",
                "-draw",
                "circle 100,125 100,50",
                path.to_str().unwrap(),
            ])
            .output()
            .unwrap();
    }

    #[test]
    #[ignore] // Requires ImageMagick
    fn process_generates_outputs() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let output_dir = tmp.path().join("output");

        // Create test image
        let image_path = source_dir.join("test-album/001-test.jpg");
        create_test_image(&image_path);

        // Create manifest
        let manifest_path = create_test_manifest(tmp.path());

        // Process
        let config = ProcessConfig {
            sizes: vec![100, 150],
            thumbnail_size: 80,
            ..Default::default()
        };

        let result = process(&manifest_path, &source_dir, &output_dir, &config).unwrap();

        // Verify outputs exist
        assert_eq!(result.albums.len(), 1);
        assert_eq!(result.albums[0].images.len(), 1);

        let image = &result.albums[0].images[0];
        assert!(!image.generated.is_empty());
        assert!(!image.thumbnail.is_empty());

        // Check files were created
        let album_dir = output_dir.join("test-album");
        assert!(album_dir.join("001-test-thumb.webp").exists());
    }

    #[test]
    #[ignore] // Requires ImageMagick
    fn thumbnail_has_correct_aspect() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let output_dir = tmp.path().join("output");

        let image_path = source_dir.join("test-album/001-test.jpg");
        create_test_image(&image_path);

        let manifest_path = create_test_manifest(tmp.path());

        let config = ProcessConfig {
            sizes: vec![100],
            thumbnail_size: 80,
            thumbnail_aspect: (4, 5),
            ..Default::default()
        };

        process(&manifest_path, &source_dir, &output_dir, &config).unwrap();

        // Check thumbnail dimensions
        let thumb_path = output_dir.join("test-album/001-test-thumb.webp");
        let backend = ImageMagickBackend::new();
        let dims = crate::imaging::get_dimensions(&backend, &thumb_path).unwrap();

        // Should be 80x100 (4:5 with short edge 80)
        assert_eq!(dims, (80, 100));
    }
}
