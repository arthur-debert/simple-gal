//! Image processing and responsive image generation.
//!
//! Stage 2 of the Simple Gal build pipeline. Takes the manifest from the scan stage
//! and processes all images to generate responsive sizes and thumbnails.
//!
//! ## Dependencies
//!
//! Uses the pure Rust imaging backend — no external dependencies required.
//!
//! ## Output Formats
//!
//! For each source image, generates:
//! - **Responsive images**: Multiple sizes in AVIF format
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
//! │   ├── 001-dawn-1400.avif
//! │   ├── 001-dawn-2080.avif
//! │   └── 001-dawn-thumb.avif    # 4:5 center-cropped thumbnail
//! └── ...
//! ```
//!
use crate::cache::{self, CacheManifest, CacheStats};
use crate::config::SiteConfig;
use crate::imaging::{
    BackendError, ImageBackend, Quality, ResponsiveConfig, RustBackend, Sharpening,
    ThumbnailConfig, get_dimensions,
};
use crate::metadata;
use crate::types::{NavItem, Page};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::Sender;
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
            thumbnail_size: config.thumbnails.size,
        }
    }
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self::from_site_config(&SiteConfig::default())
    }
}

/// Input manifest (from scan stage)
#[derive(Debug, Deserialize)]
pub struct InputManifest {
    pub navigation: Vec<NavItem>,
    pub albums: Vec<InputAlbum>,
    #[serde(default)]
    pub pages: Vec<Page>,
    #[serde(default)]
    pub description: Option<String>,
    pub config: SiteConfig,
}

#[derive(Debug, Deserialize)]
pub struct InputAlbum {
    pub path: String,
    pub title: String,
    pub description: Option<String>,
    pub preview_image: String,
    pub images: Vec<InputImage>,
    pub in_nav: bool,
    pub config: SiteConfig,
    #[serde(default)]
    pub support_files: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct InputImage {
    pub number: u32,
    pub source_path: String,
    pub filename: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Output manifest (after processing)
#[derive(Debug, Serialize)]
pub struct OutputManifest {
    pub navigation: Vec<NavItem>,
    pub albums: Vec<OutputAlbum>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<Page>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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
    pub config: SiteConfig,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub support_files: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct OutputImage {
    pub number: u32,
    pub source_path: String,
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Original dimensions (width, height)
    pub dimensions: (u32, u32),
    /// Generated responsive images: { "800": { "avif": "path" }, ... }
    pub generated: std::collections::BTreeMap<String, GeneratedVariant>,
    /// Thumbnail path
    pub thumbnail: String,
}

#[derive(Debug, Serialize)]
pub struct GeneratedVariant {
    pub avif: String,
    pub width: u32,
    pub height: u32,
}

/// Process result containing the output manifest and cache statistics.
pub struct ProcessResult {
    pub manifest: OutputManifest,
    pub cache_stats: CacheStats,
}

/// Progress events emitted during image processing.
///
/// Sent through an optional channel so callers can display progress
/// as images complete, without the process module touching stdout.
#[derive(Debug, Clone)]
pub enum ProcessEvent {
    /// An album is about to be processed.
    AlbumStarted { title: String, image_count: usize },
    /// A single image finished processing (or served from cache).
    ImageProcessed { title: String, sizes: Vec<String> },
}

pub fn process(
    manifest_path: &Path,
    source_root: &Path,
    output_dir: &Path,
    use_cache: bool,
    progress: Option<Sender<ProcessEvent>>,
) -> Result<ProcessResult, ProcessError> {
    let backend = RustBackend::new();
    process_with_backend(
        &backend,
        manifest_path,
        source_root,
        output_dir,
        use_cache,
        progress,
    )
}

/// Process images using a specific backend (allows testing with mock).
pub fn process_with_backend(
    backend: &impl ImageBackend,
    manifest_path: &Path,
    source_root: &Path,
    output_dir: &Path,
    use_cache: bool,
    progress: Option<Sender<ProcessEvent>>,
) -> Result<ProcessResult, ProcessError> {
    let manifest_content = std::fs::read_to_string(manifest_path)?;
    let input: InputManifest = serde_json::from_str(&manifest_content)?;

    std::fs::create_dir_all(output_dir)?;

    let cache = Mutex::new(if use_cache {
        CacheManifest::load(output_dir)
    } else {
        CacheManifest::empty()
    });
    let stats = Mutex::new(CacheStats::default());

    let mut output_albums = Vec::new();

    for album in &input.albums {
        if let Some(ref tx) = progress {
            tx.send(ProcessEvent::AlbumStarted {
                title: album.title.clone(),
                image_count: album.images.len(),
            })
            .ok();
        }

        // Per-album config from the resolved config chain
        let album_process = ProcessConfig::from_site_config(&album.config);

        let responsive_config = ResponsiveConfig {
            sizes: album_process.sizes.clone(),
            quality: Quality::new(album_process.quality),
        };

        let thumbnail_config = ThumbnailConfig {
            aspect: album_process.thumbnail_aspect,
            short_edge: album_process.thumbnail_size,
            quality: Quality::new(album_process.quality),
            sharpening: Some(Sharpening::light()),
        };
        let album_output_dir = output_dir.join(&album.path);
        std::fs::create_dir_all(&album_output_dir)?;

        // Process images in parallel (rayon thread pool sized by config)
        let processed_images: Result<Vec<_>, ProcessError> = album
            .images
            .par_iter()
            .map(|image| {
                let source_path = source_root.join(&image.source_path);
                if !source_path.exists() {
                    return Err(ProcessError::SourceNotFound(source_path));
                }

                let dimensions = get_dimensions(backend, &source_path)?;

                // Read embedded IPTC metadata and merge with scan-phase values.
                // This always runs so metadata changes are never stale.
                let exif = backend.read_metadata(&source_path)?;
                let title = metadata::resolve(&[exif.title.as_deref(), image.title.as_deref()]);
                let description =
                    metadata::resolve(&[image.description.as_deref(), exif.description.as_deref()]);
                let slug = if exif.title.is_some() && title.is_some() {
                    metadata::sanitize_slug(title.as_deref().unwrap())
                } else {
                    image.slug.clone()
                };

                let stem = Path::new(&image.filename)
                    .file_stem()
                    .unwrap()
                    .to_str()
                    .unwrap();

                // Compute source hash once, shared across all variants
                let source_hash = cache::hash_file(&source_path)?;
                let ctx = CacheContext {
                    source_hash: &source_hash,
                    cache: &cache,
                    stats: &stats,
                    cache_root: output_dir,
                };

                let variants = create_responsive_images_cached(
                    backend,
                    &source_path,
                    &album_output_dir,
                    stem,
                    dimensions,
                    &responsive_config,
                    &ctx,
                )?;

                let thumbnail_path = create_thumbnail_cached(
                    backend,
                    &source_path,
                    &album_output_dir,
                    stem,
                    &thumbnail_config,
                    &ctx,
                )?;

                let generated: std::collections::BTreeMap<String, GeneratedVariant> = variants
                    .into_iter()
                    .map(|v| {
                        (
                            v.target_size.to_string(),
                            GeneratedVariant {
                                avif: v.avif_path,
                                width: v.width,
                                height: v.height,
                            },
                        )
                    })
                    .collect();

                if let Some(ref tx) = progress {
                    let display_title = title
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .or_else(|| Some(slug.as_str()).filter(|s| !s.is_empty()))
                        .unwrap_or(stem)
                        .to_string();
                    let sizes: Vec<String> = generated.keys().cloned().collect();
                    tx.send(ProcessEvent::ImageProcessed {
                        title: display_title,
                        sizes,
                    })
                    .ok();
                }

                Ok((
                    image,
                    dimensions,
                    generated,
                    thumbnail_path,
                    title,
                    description,
                    slug,
                ))
            })
            .collect();
        let processed_images = processed_images?;

        // Build output images (preserving order)
        let mut output_images: Vec<OutputImage> = processed_images
            .into_iter()
            .map(
                |(image, dimensions, generated, thumbnail_path, title, description, slug)| {
                    OutputImage {
                        number: image.number,
                        source_path: image.source_path.clone(),
                        slug,
                        title,
                        description,
                        dimensions,
                        generated,
                        thumbnail: thumbnail_path,
                    }
                },
            )
            .collect();

        // Sort by number to ensure consistent ordering
        output_images.sort_by_key(|img| img.number);

        // Find album thumbnail: match the preview_image from scan, fall back to first.
        // If the preview_image is a dedicated thumb file (not in the image list),
        // generate just a thumbnail for it.
        let album_thumbnail = if let Some(img) = output_images
            .iter()
            .find(|img| img.source_path == album.preview_image)
        {
            img.thumbnail.clone()
        } else {
            // Dedicated thumb file — process only its thumbnail
            let thumb_source = source_root.join(&album.preview_image);
            let stem = Path::new(&album.preview_image)
                .file_stem()
                .unwrap()
                .to_str()
                .unwrap();
            let source_hash = cache::hash_file(&thumb_source)?;
            let ctx = CacheContext {
                source_hash: &source_hash,
                cache: &cache,
                stats: &stats,
                cache_root: output_dir,
            };
            create_thumbnail_cached(
                backend,
                &thumb_source,
                &album_output_dir,
                stem,
                &thumbnail_config,
                &ctx,
            )?
        };

        output_albums.push(OutputAlbum {
            path: album.path.clone(),
            title: album.title.clone(),
            description: album.description.clone(),
            preview_image: album.preview_image.clone(),
            thumbnail: album_thumbnail,
            images: output_images,
            in_nav: album.in_nav,
            config: album.config.clone(),
            support_files: album.support_files.clone(),
        });
    }

    let final_stats = stats.into_inner().unwrap();
    cache.into_inner().unwrap().save(output_dir)?;

    Ok(ProcessResult {
        manifest: OutputManifest {
            navigation: input.navigation,
            albums: output_albums,
            pages: input.pages,
            description: input.description,
            config: input.config,
        },
        cache_stats: final_stats,
    })
}

/// Create responsive images with cache awareness.
///
/// For each variant, checks the cache before encoding. On a cache hit the
/// existing output file is reused and no backend call is made.
/// Shared cache state passed to per-image encoding functions.
struct CacheContext<'a> {
    source_hash: &'a str,
    cache: &'a Mutex<CacheManifest>,
    stats: &'a Mutex<CacheStats>,
    cache_root: &'a Path,
}

/// Create responsive images with cache awareness.
///
/// For each variant, checks the cache before encoding. On a cache hit the
/// existing output file is reused and no backend call is made.
fn create_responsive_images_cached(
    backend: &impl ImageBackend,
    source: &Path,
    output_dir: &Path,
    filename_stem: &str,
    original_dims: (u32, u32),
    config: &ResponsiveConfig,
    ctx: &CacheContext<'_>,
) -> Result<Vec<crate::imaging::operations::GeneratedVariant>, ProcessError> {
    use crate::imaging::calculations::calculate_responsive_sizes;

    let sizes = calculate_responsive_sizes(original_dims, &config.sizes);
    let mut variants = Vec::new();

    let relative_dir = output_dir
        .file_name()
        .map(|s| s.to_str().unwrap())
        .unwrap_or("");

    for size in sizes {
        let avif_name = format!("{}-{}.avif", filename_stem, size.target);
        let relative_path = format!("{}/{}", relative_dir, avif_name);
        let params_hash = cache::hash_responsive_params(size.target, config.quality.value());

        let is_hit = ctx.cache.lock().unwrap().is_cached(
            &relative_path,
            ctx.source_hash,
            &params_hash,
            ctx.cache_root,
        );

        if is_hit {
            ctx.stats.lock().unwrap().hit();
        } else {
            let avif_path = output_dir.join(&avif_name);
            backend.resize(&crate::imaging::params::ResizeParams {
                source: source.to_path_buf(),
                output: avif_path,
                width: size.width,
                height: size.height,
                quality: config.quality,
            })?;
            let mut c = ctx.cache.lock().unwrap();
            c.insert(
                relative_path.clone(),
                ctx.source_hash.to_string(),
                params_hash,
            );
            ctx.stats.lock().unwrap().miss();
        }

        variants.push(crate::imaging::operations::GeneratedVariant {
            target_size: size.target,
            avif_path: relative_path,
            width: size.width,
            height: size.height,
        });
    }

    Ok(variants)
}

/// Create a thumbnail with cache awareness.
fn create_thumbnail_cached(
    backend: &impl ImageBackend,
    source: &Path,
    output_dir: &Path,
    filename_stem: &str,
    config: &ThumbnailConfig,
    ctx: &CacheContext<'_>,
) -> Result<String, ProcessError> {
    let thumb_name = format!("{}-thumb.avif", filename_stem);
    let relative_dir = output_dir
        .file_name()
        .map(|s| s.to_str().unwrap())
        .unwrap_or("");
    let relative_path = format!("{}/{}", relative_dir, thumb_name);

    let sharpening_tuple = config.sharpening.map(|s| (s.sigma, s.threshold));
    let params_hash = cache::hash_thumbnail_params(
        config.aspect,
        config.short_edge,
        config.quality.value(),
        sharpening_tuple,
    );

    let is_hit = ctx.cache.lock().unwrap().is_cached(
        &relative_path,
        ctx.source_hash,
        &params_hash,
        ctx.cache_root,
    );

    if is_hit {
        ctx.stats.lock().unwrap().hit();
    } else {
        let thumb_path = output_dir.join(&thumb_name);
        let params = crate::imaging::operations::plan_thumbnail(source, &thumb_path, config);
        backend.thumbnail(&params)?;
        let mut c = ctx.cache.lock().unwrap();
        c.insert(
            relative_path.clone(),
            ctx.source_hash.to_string(),
            params_hash,
        );
        ctx.stats.lock().unwrap().miss();
    }

    Ok(relative_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // =========================================================================
    // ProcessConfig tests
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
    // Manifest parsing tests
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
                "in_nav": true,
                "config": {}
            }],
            "pages": [{
                "title": "About",
                "link_title": "about",
                "slug": "about",
                "body": "# About\n\nContent",
                "in_nav": true,
                "sort_key": 40,
                "is_link": false
            }],
            "config": {}
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
        assert_eq!(manifest.pages.len(), 1);
        assert_eq!(manifest.pages[0].title, "About");
    }

    #[test]
    fn parse_manifest_without_pages() {
        let manifest_json = r##"{
            "navigation": [],
            "albums": [],
            "config": {}
        }"##;

        let manifest: InputManifest = serde_json::from_str(manifest_json).unwrap();
        assert!(manifest.pages.is_empty());
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
    // Process with mock backend tests
    // =========================================================================

    use crate::imaging::Dimensions;
    use crate::imaging::backend::tests::MockBackend;

    fn create_test_manifest(tmp: &Path) -> PathBuf {
        create_test_manifest_with_config(tmp, "{}")
    }

    fn create_test_manifest_with_config(tmp: &Path, album_config_json: &str) -> PathBuf {
        let manifest = format!(
            r##"{{
            "navigation": [],
            "albums": [{{
                "path": "test-album",
                "title": "Test Album",
                "description": null,
                "preview_image": "test-album/001-test.jpg",
                "images": [{{
                    "number": 1,
                    "source_path": "test-album/001-test.jpg",
                    "filename": "001-test.jpg"
                }}],
                "in_nav": true,
                "config": {album_config}
            }}],
            "config": {{}}
        }}"##,
            album_config = album_config_json,
        );

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

        // Create manifest with per-album config
        let manifest_path =
            create_test_manifest_with_config(tmp.path(), r#"{"images": {"sizes": [100, 150]}}"#);

        // Create mock backend with dimensions
        let backend = MockBackend::with_dimensions(vec![Dimensions {
            width: 200,
            height: 250,
        }]);

        let result = process_with_backend(
            &backend,
            &manifest_path,
            &source_dir,
            &output_dir,
            false,
            None,
        )
        .unwrap();

        // Verify outputs
        assert_eq!(result.manifest.albums.len(), 1);
        assert_eq!(result.manifest.albums[0].images.len(), 1);

        let image = &result.manifest.albums[0].images[0];
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

        // Per-album config with quality=85 and sizes=[800,1400]
        let manifest_path = create_test_manifest_with_config(
            tmp.path(),
            r#"{"images": {"sizes": [800, 1400], "quality": 85}}"#,
        );

        // 2000x1500 landscape - should generate both sizes
        let backend = MockBackend::with_dimensions(vec![Dimensions {
            width: 2000,
            height: 1500,
        }]);

        process_with_backend(
            &backend,
            &manifest_path,
            &source_dir,
            &output_dir,
            false,
            None,
        )
        .unwrap();

        use crate::imaging::backend::tests::RecordedOp;
        let ops = backend.get_operations();

        // Should have: 1 identify + 1 read_metadata + 2 resizes (2 sizes × AVIF) + 1 thumbnail = 5 ops
        assert_eq!(ops.len(), 5);

        // First is identify
        assert!(matches!(&ops[0], RecordedOp::Identify(_)));

        // Second is read_metadata
        assert!(matches!(&ops[1], RecordedOp::ReadMetadata(_)));

        // Then resizes with correct quality
        for op in &ops[2..4] {
            assert!(matches!(op, RecordedOp::Resize { quality: 85, .. }));
        }

        // Last is thumbnail
        assert!(matches!(&ops[4], RecordedOp::Thumbnail { .. }));
    }

    #[test]
    fn process_with_mock_skips_larger_sizes() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let output_dir = tmp.path().join("output");

        let image_path = source_dir.join("test-album/001-test.jpg");
        create_dummy_source(&image_path);

        // Per-album config with sizes larger than the source image
        let manifest_path = create_test_manifest_with_config(
            tmp.path(),
            r#"{"images": {"sizes": [800, 1400, 2080]}}"#,
        );

        // 500x400 - smaller than all requested sizes
        let backend = MockBackend::with_dimensions(vec![Dimensions {
            width: 500,
            height: 400,
        }]);

        let result = process_with_backend(
            &backend,
            &manifest_path,
            &source_dir,
            &output_dir,
            false,
            None,
        )
        .unwrap();

        // Should only have original size
        let image = &result.manifest.albums[0].images[0];
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

        let result = process_with_backend(
            &backend,
            &manifest_path,
            &source_dir,
            &output_dir,
            false,
            None,
        );

        assert!(matches!(result, Err(ProcessError::SourceNotFound(_))));
    }

    // =========================================================================
    // Cache integration tests
    // =========================================================================

    /// Helper: run process with cache enabled, returning (ops_count, cache_stats).
    fn run_cached(
        source_dir: &Path,
        output_dir: &Path,
        manifest_path: &Path,
        dims: Vec<Dimensions>,
    ) -> (Vec<crate::imaging::backend::tests::RecordedOp>, CacheStats) {
        let backend = MockBackend::with_dimensions(dims);
        let result =
            process_with_backend(&backend, manifest_path, source_dir, output_dir, true, None)
                .unwrap();
        (backend.get_operations(), result.cache_stats)
    }

    #[test]
    fn cache_second_run_skips_all_encoding() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let output_dir = tmp.path().join("output");

        let image_path = source_dir.join("test-album/001-test.jpg");
        create_dummy_source(&image_path);

        let manifest_path = create_test_manifest_with_config(
            tmp.path(),
            r#"{"images": {"sizes": [800, 1400], "quality": 85}}"#,
        );

        // First run: everything is a miss
        let (_ops1, stats1) = run_cached(
            &source_dir,
            &output_dir,
            &manifest_path,
            vec![Dimensions {
                width: 2000,
                height: 1500,
            }],
        );

        // The mock backend doesn't write real files, so we need to create
        // dummy output files for the cache hit check on the second run.
        for entry in cache::CacheManifest::load(&output_dir).entries.keys() {
            let path = output_dir.join(entry);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "fake avif").unwrap();
        }

        // Second run: everything should be a cache hit
        let (ops2, stats2) = run_cached(
            &source_dir,
            &output_dir,
            &manifest_path,
            vec![Dimensions {
                width: 2000,
                height: 1500,
            }],
        );

        // First run: 2 resizes + 1 thumbnail = 3 misses
        assert_eq!(stats1.misses, 3);
        assert_eq!(stats1.hits, 0);

        // Second run: 0 resizes + 0 thumbnails encoded, all cached
        assert_eq!(stats2.hits, 3);
        assert_eq!(stats2.misses, 0);

        // Second run should only have identify + read_metadata (no resize/thumbnail)
        use crate::imaging::backend::tests::RecordedOp;
        let encode_ops: Vec<_> = ops2
            .iter()
            .filter(|op| matches!(op, RecordedOp::Resize { .. } | RecordedOp::Thumbnail { .. }))
            .collect();
        assert_eq!(encode_ops.len(), 0);
    }

    #[test]
    fn cache_invalidated_when_source_changes() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let output_dir = tmp.path().join("output");

        let image_path = source_dir.join("test-album/001-test.jpg");
        create_dummy_source(&image_path);

        let manifest_path =
            create_test_manifest_with_config(tmp.path(), r#"{"images": {"sizes": [800]}}"#);

        // First run
        let (_ops1, stats1) = run_cached(
            &source_dir,
            &output_dir,
            &manifest_path,
            vec![Dimensions {
                width: 2000,
                height: 1500,
            }],
        );
        assert_eq!(stats1.misses, 2); // 1 resize + 1 thumb

        // Create dummy outputs
        for entry in cache::CacheManifest::load(&output_dir).entries.keys() {
            let path = output_dir.join(entry);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "fake").unwrap();
        }

        // Modify source file content (changes source_hash)
        fs::write(&image_path, "different content").unwrap();

        // Second run: cache should miss because source hash changed
        let (_ops2, stats2) = run_cached(
            &source_dir,
            &output_dir,
            &manifest_path,
            vec![Dimensions {
                width: 2000,
                height: 1500,
            }],
        );
        assert_eq!(stats2.misses, 2);
        assert_eq!(stats2.hits, 0);
    }

    #[test]
    fn cache_invalidated_when_config_changes() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let output_dir = tmp.path().join("output");

        let image_path = source_dir.join("test-album/001-test.jpg");
        create_dummy_source(&image_path);

        // First run with quality=85
        let manifest_path = create_test_manifest_with_config(
            tmp.path(),
            r#"{"images": {"sizes": [800], "quality": 85}}"#,
        );
        let (_ops1, stats1) = run_cached(
            &source_dir,
            &output_dir,
            &manifest_path,
            vec![Dimensions {
                width: 2000,
                height: 1500,
            }],
        );
        assert_eq!(stats1.misses, 2);

        // Create dummy outputs
        for entry in cache::CacheManifest::load(&output_dir).entries.keys() {
            let path = output_dir.join(entry);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "fake").unwrap();
        }

        // Second run with quality=90 — params_hash changes, cache invalidated
        let manifest_path = create_test_manifest_with_config(
            tmp.path(),
            r#"{"images": {"sizes": [800], "quality": 90}}"#,
        );
        let (_ops2, stats2) = run_cached(
            &source_dir,
            &output_dir,
            &manifest_path,
            vec![Dimensions {
                width: 2000,
                height: 1500,
            }],
        );
        assert_eq!(stats2.misses, 2);
        assert_eq!(stats2.hits, 0);
    }

    #[test]
    fn no_cache_flag_forces_full_reprocess() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let output_dir = tmp.path().join("output");

        let image_path = source_dir.join("test-album/001-test.jpg");
        create_dummy_source(&image_path);

        let manifest_path =
            create_test_manifest_with_config(tmp.path(), r#"{"images": {"sizes": [800]}}"#);

        // First run with cache
        let (_ops1, _stats1) = run_cached(
            &source_dir,
            &output_dir,
            &manifest_path,
            vec![Dimensions {
                width: 2000,
                height: 1500,
            }],
        );

        // Create dummy outputs
        for entry in cache::CacheManifest::load(&output_dir).entries.keys() {
            let path = output_dir.join(entry);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "fake").unwrap();
        }

        // Second run with use_cache=false (simulates --no-cache)
        let backend = MockBackend::with_dimensions(vec![Dimensions {
            width: 2000,
            height: 1500,
        }]);
        let result = process_with_backend(
            &backend,
            &manifest_path,
            &source_dir,
            &output_dir,
            false,
            None,
        )
        .unwrap();

        // Should re-encode everything despite outputs existing
        assert_eq!(result.cache_stats.misses, 2);
        assert_eq!(result.cache_stats.hits, 0);
    }
}
