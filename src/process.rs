//! Image processing and responsive image generation.
//!
//! Stage 2 of the LightTable build pipeline. Takes the manifest from the scan stage
//! and processes all images to generate responsive sizes and thumbnails.
//!
//! ## Dependencies
//!
//! Requires ImageMagick to be installed:
//! - **ImageMagick 7** (preferred): Uses the `magick` command
//! - **ImageMagick 6** (fallback): Uses the `convert` command
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
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProcessError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("ImageMagick failed: {0}")]
    ImageMagick(String),
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

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            sizes: vec![800, 1400, 2080],
            quality: 90,
            thumbnail_aspect: (4, 5),
            thumbnail_size: 400,
        }
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
    let manifest_content = std::fs::read_to_string(manifest_path)?;
    let input: InputManifest = serde_json::from_str(&manifest_content)?;

    std::fs::create_dir_all(output_dir)?;

    let mut output_albums = Vec::new();

    for album in &input.albums {
        println!("Processing album: {}", album.title);
        let album_output_dir = output_dir.join(&album.path);
        std::fs::create_dir_all(&album_output_dir)?;

        // Process images in parallel
        let results: Result<Vec<_>, _> = album
            .images
            .par_iter()
            .map(|image| {
                let source_path = source_root.join(&image.source_path);
                if !source_path.exists() {
                    return Err(ProcessError::SourceNotFound(source_path));
                }

                println!("  {} ", image.filename);

                // Get original dimensions
                let dimensions = get_dimensions(&source_path)?;

                // Generate responsive sizes
                let generated = generate_responsive_images(
                    &source_path,
                    &album_output_dir,
                    &image.filename,
                    dimensions,
                    config,
                )?;

                // Generate thumbnail
                let thumbnail_path = generate_thumbnail(
                    &source_path,
                    &album_output_dir,
                    &image.filename,
                    config,
                )?;

                Ok((image, dimensions, generated, thumbnail_path))
            })
            .collect();

        let processed_images = results?;

        // Build output images (preserving order)
        let mut output_images: Vec<OutputImage> = processed_images
            .into_iter()
            .map(|(image, dimensions, generated, thumbnail_path)| OutputImage {
                number: image.number,
                source_path: image.source_path.clone(),
                dimensions,
                generated,
                thumbnail: thumbnail_path,
            })
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

fn get_dimensions(path: &Path) -> Result<(u32, u32), ProcessError> {
    let cmd = get_imagemagick_command();
    let args = if cmd == "magick" {
        vec!["identify", "-format", "%w %h", path.to_str().unwrap()]
    } else {
        vec!["-format", "%w %h", path.to_str().unwrap()]
    };
    // Use identify command for dimensions
    let output = Command::new(if cmd == "magick" { "magick" } else { "identify" })
        .args(&args)
        .output()?;

    if !output.status.success() {
        return Err(ProcessError::ImageMagick(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    let dims = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = dims.split_whitespace().collect();
    if parts.len() != 2 {
        return Err(ProcessError::ImageMagick(format!(
            "Unexpected identify output: {}",
            dims
        )));
    }

    let width: u32 = parts[0].parse().map_err(|_| {
        ProcessError::ImageMagick(format!("Invalid width: {}", parts[0]))
    })?;
    let height: u32 = parts[1].parse().map_err(|_| {
        ProcessError::ImageMagick(format!("Invalid height: {}", parts[1]))
    })?;

    Ok((width, height))
}

fn generate_responsive_images(
    source: &Path,
    output_dir: &Path,
    filename: &str,
    dimensions: (u32, u32),
    config: &ProcessConfig,
) -> Result<std::collections::BTreeMap<String, GeneratedVariant>, ProcessError> {
    let mut generated = std::collections::BTreeMap::new();
    let stem = Path::new(filename).file_stem().unwrap().to_str().unwrap();

    let (orig_w, orig_h) = dimensions;
    let longer_edge = orig_w.max(orig_h);

    for &target_size in &config.sizes {
        // Skip sizes larger than original
        if target_size > longer_edge {
            continue;
        }

        // Calculate output dimensions (preserve aspect ratio)
        let (out_w, out_h) = if orig_w >= orig_h {
            let ratio = target_size as f64 / orig_w as f64;
            (target_size, (orig_h as f64 * ratio).round() as u32)
        } else {
            let ratio = target_size as f64 / orig_h as f64;
            ((orig_w as f64 * ratio).round() as u32, target_size)
        };

        let avif_name = format!("{}-{}.avif", stem, target_size);
        let webp_name = format!("{}-{}.webp", stem, target_size);
        let avif_path = output_dir.join(&avif_name);
        let webp_path = output_dir.join(&webp_name);

        // Generate AVIF
        run_magick(&[
            source.to_str().unwrap(),
            "-resize",
            &format!("{}x{}", out_w, out_h),
            "-quality",
            &config.quality.to_string(),
            "-define",
            "heic:speed=6", // Faster encoding
            avif_path.to_str().unwrap(),
        ])?;

        // Generate WebP
        run_magick(&[
            source.to_str().unwrap(),
            "-resize",
            &format!("{}x{}", out_w, out_h),
            "-quality",
            &config.quality.to_string(),
            webp_path.to_str().unwrap(),
        ])?;

        let relative_dir = output_dir
            .file_name()
            .map(|s| s.to_str().unwrap())
            .unwrap_or("");

        generated.insert(
            target_size.to_string(),
            GeneratedVariant {
                avif: format!("{}/{}", relative_dir, avif_name),
                webp: format!("{}/{}", relative_dir, webp_name),
                width: out_w,
                height: out_h,
            },
        );
    }

    // If original is smaller than smallest target, use original size
    if generated.is_empty() {
        let avif_name = format!("{}-{}.avif", stem, longer_edge);
        let webp_name = format!("{}-{}.webp", stem, longer_edge);
        let avif_path = output_dir.join(&avif_name);
        let webp_path = output_dir.join(&webp_name);

        run_magick(&[
            source.to_str().unwrap(),
            "-quality",
            &config.quality.to_string(),
            avif_path.to_str().unwrap(),
        ])?;

        run_magick(&[
            source.to_str().unwrap(),
            "-quality",
            &config.quality.to_string(),
            webp_path.to_str().unwrap(),
        ])?;

        let relative_dir = output_dir
            .file_name()
            .map(|s| s.to_str().unwrap())
            .unwrap_or("");

        generated.insert(
            longer_edge.to_string(),
            GeneratedVariant {
                avif: format!("{}/{}", relative_dir, avif_name),
                webp: format!("{}/{}", relative_dir, webp_name),
                width: orig_w,
                height: orig_h,
            },
        );
    }

    Ok(generated)
}

fn generate_thumbnail(
    source: &Path,
    output_dir: &Path,
    filename: &str,
    config: &ProcessConfig,
) -> Result<String, ProcessError> {
    let stem = Path::new(filename).file_stem().unwrap().to_str().unwrap();
    let (aspect_w, aspect_h) = config.thumbnail_aspect;

    // Calculate thumbnail dimensions based on aspect ratio
    // If aspect is 4:5 (portrait), width is short edge
    let (thumb_w, thumb_h) = if aspect_w <= aspect_h {
        let w = config.thumbnail_size;
        let h = (w as f64 * aspect_h as f64 / aspect_w as f64).round() as u32;
        (w, h)
    } else {
        let h = config.thumbnail_size;
        let w = (h as f64 * aspect_w as f64 / aspect_h as f64).round() as u32;
        (w, h)
    };

    let thumb_name = format!("{}-thumb.webp", stem);
    let thumb_path = output_dir.join(&thumb_name);

    // Use ImageMagick to resize and crop to fill the thumbnail
    // -resize WxH^ resizes to fill (may exceed dimensions)
    // -gravity center -extent WxH crops to exact size
    run_magick(&[
        source.to_str().unwrap(),
        "-resize",
        &format!("{}x{}^", thumb_w, thumb_h),
        "-gravity",
        "center",
        "-extent",
        &format!("{}x{}", thumb_w, thumb_h),
        "-quality",
        &config.quality.to_string(),
        "-sharpen",
        "0x0.5", // Light sharpening for thumbnails
        thumb_path.to_str().unwrap(),
    ])?;

    let relative_dir = output_dir
        .file_name()
        .map(|s| s.to_str().unwrap())
        .unwrap_or("");

    Ok(format!("{}/{}", relative_dir, thumb_name))
}

fn get_imagemagick_command() -> &'static str {
    use std::sync::OnceLock;
    static CMD: OnceLock<&str> = OnceLock::new();
    CMD.get_or_init(|| {
        // Prefer magick (ImageMagick 7) but fall back to convert (ImageMagick 6)
        if Command::new("magick").arg("-version").output().is_ok_and(|o| o.status.success()) {
            "magick"
        } else {
            "convert"
        }
    })
}

fn run_magick(args: &[&str]) -> Result<(), ProcessError> {
    let cmd = get_imagemagick_command();
    let output = Command::new(cmd).args(args).output()?;

    if !output.status.success() {
        return Err(ProcessError::ImageMagick(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_manifest(tmp: &Path) -> PathBuf {
        let manifest = r#"{
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
            }]
        }"#;

        let manifest_path = tmp.join("manifest.json");
        fs::write(&manifest_path, manifest).unwrap();
        manifest_path
    }

    fn create_test_image(path: &Path) {
        // Create a 200x250 test image (4:5 aspect)
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        Command::new("magick")
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
        let dims = get_dimensions(&thumb_path).unwrap();

        // Should be 80x100 (4:5 with short edge 80)
        assert_eq!(dims, (80, 100));
    }
}
