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
    /// The on-disk cache manifest's `schema_version` doesn't match the
    /// one this binary was built with. Phase 4a of the data-model
    /// refactor made this loud rather than silent — see §5.2 of the
    /// refactor plan. Callers should tell the user to remove the
    /// processed directory (or pass `--auto-reset-cache` to have the
    /// CLI do it).
    #[error(transparent)]
    CacheSchemaMismatch(#[from] cache::CacheLoadError),
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
    /// Flat canonical view of images keyed by content hash (Phase 1 of
    /// the data-model refactor). Each entry here is a unique byte
    /// stream; [`InputImage::canonical_id`] points into this list. Old
    /// manifests without the field deserialize with an empty list —
    /// the per-album path-based flow still works.
    #[serde(default)]
    pub canonical_images: Vec<InputCanonicalImage>,
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
    /// Pointer into [`InputManifest::canonical_images`]. Populated by
    /// scan for manifests produced in v0.19.x or later; absent on
    /// older ones (back-compat path falls through to the ref's own
    /// `source_path`).
    #[serde(default)]
    pub canonical_id: Option<String>,
}

/// Mirror of `scan::CanonicalImage`, serialized through the manifest.
#[derive(Debug, Deserialize)]
pub struct InputCanonicalImage {
    pub id: String,
    pub source_path: String,
    #[serde(default)]
    pub aliases: Vec<String>,
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
    /// Canonical-image view threaded through from scan (Phase 1).
    /// Forwarded as-is for downstream metadata resolution and future
    /// consumers; the current All Photos dedup path is driven by
    /// `OutputImage::canonical_id` while walking album images. Empty on
    /// legacy pre-Phase-1 manifests.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub canonical_images: Vec<OutputCanonicalImage>,
}

/// Mirror of [`scan::CanonicalImage`] serialized through the processed
/// manifest so downstream stages retain the flat canonical-image view.
#[derive(Debug, Serialize)]
pub struct OutputCanonicalImage {
    pub id: String,
    pub source_path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
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
    /// Extra thumbnail generated for the site-wide "All Photos" page, when
    /// `[full_index] generates = true`. Uses full_index.thumb_ratio/thumb_size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_index_thumbnail: Option<String>,
    /// Pointer into [`OutputManifest::canonical_images`]. Forwarded from
    /// scan → process so generate can resolve shared-content metadata.
    /// Absent on legacy manifests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_id: Option<String>,
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
    /// Source-hash dedup stats from the canonical-image lookup (Phase 2
    /// of the data-model refactor). `unique` counts distinct source
    /// byte streams that required a fresh `hash_file` read; `reused`
    /// counts how many times a subsequent `InputImage` reused a
    /// previously-computed hash — one reuse per extra album that
    /// references the same canonical content.
    pub source_hash_stats: SourceHashStats,
}

/// Counts of source-hash dedup across a processing run.
#[derive(Debug, Default, Clone, Copy)]
pub struct SourceHashStats {
    /// Distinct source files that were hashed (one per canonical image
    /// actually touched, or per ref with no canonical_id).
    pub unique: u32,
    /// Times a previously-computed hash was reused — i.e. how many
    /// redundant reads the new canonical view avoided.
    pub reused: u32,
}

/// Cache outcome for a single processed variant (for progress reporting).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VariantStatus {
    /// Existing cached file was reused in place.
    Cached,
    /// Cached content found at a different path and copied.
    Copied,
    /// No cache entry — image was encoded from scratch.
    Encoded,
}

impl From<&CacheLookup> for VariantStatus {
    fn from(lookup: &CacheLookup) -> Self {
        match lookup {
            CacheLookup::ExactHit => VariantStatus::Cached,
            CacheLookup::Copied => VariantStatus::Copied,
            CacheLookup::Miss => VariantStatus::Encoded,
        }
    }
}

/// Information about a single processed variant (for progress reporting).
#[derive(Debug, Clone, Serialize)]
pub struct VariantInfo {
    /// Display label (e.g., "800px", "thumbnail").
    pub label: String,
    /// Whether this variant was cached, copied, or encoded.
    pub status: VariantStatus,
}

/// Progress events emitted during image processing.
///
/// Sent through an optional channel so callers can display progress
/// as images complete, without the process module touching stdout.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ProcessEvent {
    /// An album is about to be processed.
    AlbumStarted { title: String, image_count: usize },
    /// A single image finished processing (or served from cache).
    ImageProcessed {
        /// 1-based positional index within the album.
        index: usize,
        /// Title if the image has one (from IPTC or filename). `None` for
        /// untitled images like `38.avif` — the output formatter shows
        /// the filename instead.
        title: Option<String>,
        /// Relative source path (e.g., "010-Landscapes/001-dawn.jpg").
        source_path: String,
        /// Per-variant cache/encode status.
        variants: Vec<VariantInfo>,
    },
    /// Stale cache entries were pruned after processing.
    CachePruned { removed: u32 },
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

    // Strict load: a schema-version mismatch surfaces as `ProcessError::CacheSchemaMismatch`
    // so the CLI can tell the user to wipe the processed dir (or pass
    // --auto-reset-cache). Other load errors (missing file → empty; IO /
    // corruption → error) behave as documented on `CacheLoadError`.
    let cache = Mutex::new(if use_cache {
        CacheManifest::load_strict(output_dir)?
    } else {
        CacheManifest::empty()
    });
    let stats = Mutex::new(CacheStats::default());

    // Canonical-view lookup: map `ImageId` → `InputCanonicalImage` for
    // O(1) resolution from per-album refs. Empty on legacy manifests
    // (pre-Phase-1 scans) — the fallback path keys the hash memo on
    // `InputImage.source_path` instead.
    let canonical_by_id: std::collections::HashMap<&str, &InputCanonicalImage> = input
        .canonical_images
        .iter()
        .map(|c| (c.id.as_str(), c))
        .collect();

    // Source-hash memo. Keys are either `canonical_id` (preferred) or
    // the per-ref `source_path` (fallback for legacy manifests and for
    // refs whose `canonical_id` doesn't resolve in `canonical_by_id`).
    // Two refs to the same canonical content hit the memo and skip the
    // second `hash_file` call entirely, avoiding the redundant disk
    // read the Phase 1 CHANGELOG flagged as the current-model cost.
    //
    // Each entry is a per-key `Arc<Mutex<Option<String>>>` so concurrent
    // rayon workers on the same key serialize on that inner mutex
    // (one worker computes, others wait and read the cached value).
    // The outer mutex only guards the map itself — brief.
    type HashCell = std::sync::Arc<Mutex<Option<String>>>;
    let source_hash_memo: Mutex<std::collections::HashMap<String, HashCell>> =
        Mutex::new(std::collections::HashMap::new());
    let source_hash_unique = std::sync::atomic::AtomicU32::new(0);
    let source_hash_reused = std::sync::atomic::AtomicU32::new(0);

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

        // Extra thumbnail for the site-wide "All Photos" page. Uses its own
        // ratio/size, encoded only when the feature is enabled. The cache
        // params_hash differs from the regular thumbnail so both can coexist.
        let full_index_thumbnail_config: Option<ThumbnailConfig> =
            if input.config.full_index.generates {
                let fi = &input.config.full_index;
                Some(ThumbnailConfig {
                    aspect: (fi.thumb_ratio[0], fi.thumb_ratio[1]),
                    short_edge: fi.thumb_size,
                    quality: Quality::new(album_process.quality),
                    sharpening: Some(Sharpening::light()),
                })
            } else {
                None
            };
        let album_output_dir = output_dir.join(&album.path);
        std::fs::create_dir_all(&album_output_dir)?;

        // Process images in parallel (rayon thread pool sized by config)
        let processed_images: Result<Vec<_>, ProcessError> = album
            .images
            .par_iter()
            .enumerate()
            .map(|(idx, image)| {
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

                // Compute source hash once per canonical content across the
                // whole site. The memo key is `canonical_id` when the scan
                // stage stamped one AND that id actually resolves in
                // `canonical_by_id` — otherwise we fall back to the ref's
                // own `source_path`. Guarding on resolution prevents a
                // manifest where `canonical_id` was set without a matching
                // entry (inconsistent input) from causing two genuinely
                // distinct images to share a hash.
                let canonical = image
                    .canonical_id
                    .as_deref()
                    .and_then(|id| canonical_by_id.get(id).map(|c| (id, *c)));
                let (memo_key, hash_input_path) = match canonical {
                    Some((id, c)) => (id.to_string(), source_root.join(&c.source_path)),
                    None => (image.source_path.clone(), source_path.clone()),
                };
                // Per-key cell: first worker locks it, hashes, stores; later
                // workers block on the same cell and just read the stored
                // hash. Workers on different keys run in parallel (each has
                // its own cell). The stats counter is now race-free because
                // only one thread ever reaches the miss branch per key.
                let cell: HashCell = {
                    let mut memo = source_hash_memo.lock().unwrap();
                    memo.entry(memo_key)
                        .or_insert_with(|| std::sync::Arc::new(Mutex::new(None)))
                        .clone()
                };
                let source_hash = {
                    use std::sync::atomic::Ordering;
                    let mut slot = cell.lock().unwrap();
                    match slot.clone() {
                        Some(h) => {
                            source_hash_reused.fetch_add(1, Ordering::Relaxed);
                            h
                        }
                        None => {
                            let h = cache::hash_file(&hash_input_path)?;
                            *slot = Some(h.clone());
                            source_hash_unique.fetch_add(1, Ordering::Relaxed);
                            h
                        }
                    }
                };
                let ctx = CacheContext {
                    source_hash: &source_hash,
                    cache: &cache,
                    stats: &stats,
                    cache_root: output_dir,
                };

                let (raw_variants, responsive_statuses) = create_responsive_images_cached(
                    backend,
                    &source_path,
                    &album_output_dir,
                    stem,
                    dimensions,
                    &responsive_config,
                    &ctx,
                )?;

                let (thumbnail_path, thumb_status) = create_thumbnail_cached(
                    backend,
                    &source_path,
                    &album_output_dir,
                    stem,
                    &thumbnail_config,
                    &ctx,
                )?;

                let full_index_thumb = if let Some(ref fi_cfg) = full_index_thumbnail_config {
                    let (path, status) = create_thumbnail_cached_with_suffix(
                        backend,
                        &source_path,
                        &album_output_dir,
                        stem,
                        "fi-thumb",
                        "full-index",
                        fi_cfg,
                        &ctx,
                    )?;
                    Some((path, status))
                } else {
                    None
                };

                // Build variant infos for progress event (before consuming raw_variants)
                let variant_infos: Vec<VariantInfo> = if progress.is_some() {
                    let mut infos: Vec<VariantInfo> = raw_variants
                        .iter()
                        .zip(&responsive_statuses)
                        .map(|(v, status)| VariantInfo {
                            label: format!("{}px", v.target_size),
                            status: status.clone(),
                        })
                        .collect();
                    infos.push(VariantInfo {
                        label: "thumbnail".to_string(),
                        status: thumb_status,
                    });
                    if let Some((_, ref fi_status)) = full_index_thumb {
                        infos.push(VariantInfo {
                            label: "all-photos thumbnail".to_string(),
                            status: fi_status.clone(),
                        });
                    }
                    infos
                } else {
                    Vec::new()
                };

                let generated: std::collections::BTreeMap<String, GeneratedVariant> = raw_variants
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
                    tx.send(ProcessEvent::ImageProcessed {
                        index: idx + 1,
                        title: title.clone(),
                        source_path: image.source_path.clone(),
                        variants: variant_infos,
                    })
                    .ok();
                }

                Ok((
                    image,
                    dimensions,
                    generated,
                    thumbnail_path,
                    full_index_thumb.map(|(p, _)| p),
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
                |(
                    image,
                    dimensions,
                    generated,
                    thumbnail_path,
                    full_index_thumbnail,
                    title,
                    description,
                    slug,
                )| {
                    OutputImage {
                        number: image.number,
                        source_path: image.source_path.clone(),
                        slug,
                        title,
                        description,
                        dimensions,
                        generated,
                        thumbnail: thumbnail_path,
                        full_index_thumbnail,
                        canonical_id: image.canonical_id.clone(),
                    }
                },
            )
            .collect();

        // Sort by number to ensure consistent ordering
        output_images.sort_by_key(|img| img.number);

        // Find album thumbnail: the preview_image is always in the image list.
        let album_thumbnail = output_images
            .iter()
            .find(|img| img.source_path == album.preview_image)
            .expect("preview_image must be in the image list")
            .thumbnail
            .clone();

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

    // Collect all output paths that are live in this build
    let live_paths: std::collections::HashSet<String> = output_albums
        .iter()
        .flat_map(|album| {
            let image_paths = album.images.iter().flat_map(|img| {
                let mut paths: Vec<String> =
                    img.generated.values().map(|v| v.avif.clone()).collect();
                paths.push(img.thumbnail.clone());
                if let Some(ref fi) = img.full_index_thumbnail {
                    paths.push(fi.clone());
                }
                paths
            });
            std::iter::once(album.thumbnail.clone()).chain(image_paths)
        })
        .collect();

    let mut final_cache = cache.into_inner().unwrap();
    let pruned = final_cache.prune(&live_paths, output_dir);
    let final_stats = stats.into_inner().unwrap();
    final_cache.save(output_dir)?;

    if let Some(ref tx) = progress
        && pruned > 0
    {
        tx.send(ProcessEvent::CachePruned { removed: pruned }).ok();
    }

    use std::sync::atomic::Ordering;
    let source_hash_stats = SourceHashStats {
        unique: source_hash_unique.load(Ordering::Relaxed),
        reused: source_hash_reused.load(Ordering::Relaxed),
    };

    // Forward canonical image metadata into the processed manifest.
    // This only transforms scan-stage data into the output manifest shape
    // (same fields, different type); downstream consumers use the
    // forwarded view as needed. A `From` impl would be cleaner if this
    // duplication grows.
    let canonical_images: Vec<OutputCanonicalImage> = input
        .canonical_images
        .into_iter()
        .map(|c| OutputCanonicalImage {
            id: c.id,
            source_path: c.source_path,
            aliases: c.aliases,
        })
        .collect();

    Ok(ProcessResult {
        manifest: OutputManifest {
            navigation: input.navigation,
            albums: output_albums,
            pages: input.pages,
            description: input.description,
            config: input.config,
            canonical_images,
        },
        cache_stats: final_stats,
        source_hash_stats,
    })
}

/// Shared cache state passed to per-image encoding functions.
struct CacheContext<'a> {
    source_hash: &'a str,
    cache: &'a Mutex<CacheManifest>,
    stats: &'a Mutex<CacheStats>,
    cache_root: &'a Path,
}

/// Result of checking the content-based cache.
enum CacheLookup {
    /// Same content, same path — file already in place.
    ExactHit,
    /// Same content at a different path — file copied to new location.
    Copied,
    /// No cached file available — caller must encode.
    Miss,
}

/// Check the cache and, if the content exists at a different path, copy it.
///
/// Returns `ExactHit` when the cached file is already at `expected_path`,
/// `Copied` when a file with matching content was found elsewhere and
/// copied to `expected_path`, or `Miss` when no cached version exists
/// (or the copy failed).
///
/// The cache mutex is held across the entire find+copy+insert sequence to
/// prevent a race where two threads processing swapped images clobber each
/// other's source files (Thread A copies over B's file before B reads it).
fn check_cache_and_copy(
    expected_path: &str,
    source_hash: &str,
    params_hash: &str,
    ctx: &CacheContext<'_>,
) -> CacheLookup {
    let mut cache = ctx.cache.lock().unwrap();
    let cached_path = cache.find_cached(source_hash, params_hash, ctx.cache_root);

    match cached_path {
        Some(ref stored) if stored == expected_path => CacheLookup::ExactHit,
        Some(ref stored) => {
            let old_file = ctx.cache_root.join(stored);
            let new_file = ctx.cache_root.join(expected_path);
            if let Some(parent) = new_file.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::copy(&old_file, &new_file) {
                Ok(_) => {
                    cache.insert(
                        expected_path.to_string(),
                        source_hash.to_string(),
                        params_hash.to_string(),
                    );
                    CacheLookup::Copied
                }
                Err(_) => CacheLookup::Miss,
            }
        }
        None => CacheLookup::Miss,
    }
}

/// Create responsive images with cache awareness.
///
/// For each variant, checks the cache before encoding. On a cache hit the
/// existing output file is reused (or copied from its old location if the
/// album was renamed) and no backend call is made.
fn create_responsive_images_cached(
    backend: &impl ImageBackend,
    source: &Path,
    output_dir: &Path,
    filename_stem: &str,
    original_dims: (u32, u32),
    config: &ResponsiveConfig,
    ctx: &CacheContext<'_>,
) -> Result<
    (
        Vec<crate::imaging::operations::GeneratedVariant>,
        Vec<VariantStatus>,
    ),
    ProcessError,
> {
    use crate::imaging::calculations::calculate_responsive_sizes;

    let sizes = calculate_responsive_sizes(original_dims, &config.sizes);
    let mut variants = Vec::new();
    let mut statuses = Vec::new();

    let relative_dir = output_dir
        .strip_prefix(ctx.cache_root)
        .unwrap()
        .to_str()
        .unwrap();

    for size in sizes {
        let avif_name = format!("{}-{}.avif", filename_stem, size.target);
        let relative_path = format!("{}/{}", relative_dir, avif_name);
        let params_hash = cache::hash_responsive_params(size.target, config.quality.value());

        let lookup = check_cache_and_copy(&relative_path, ctx.source_hash, &params_hash, ctx);
        match &lookup {
            CacheLookup::ExactHit => {
                ctx.stats.lock().unwrap().hit();
            }
            CacheLookup::Copied => {
                ctx.stats.lock().unwrap().copy();
            }
            CacheLookup::Miss => {
                let avif_path = output_dir.join(&avif_name);
                backend.resize(&crate::imaging::params::ResizeParams {
                    source: source.to_path_buf(),
                    output: avif_path,
                    width: size.width,
                    height: size.height,
                    quality: config.quality,
                })?;
                ctx.cache.lock().unwrap().insert(
                    relative_path.clone(),
                    ctx.source_hash.to_string(),
                    params_hash,
                );
                ctx.stats.lock().unwrap().miss();
            }
        }

        statuses.push(VariantStatus::from(&lookup));
        variants.push(crate::imaging::operations::GeneratedVariant {
            target_size: size.target,
            avif_path: relative_path,
            width: size.width,
            height: size.height,
        });
    }

    Ok((variants, statuses))
}

/// Create a thumbnail with cache awareness.
fn create_thumbnail_cached(
    backend: &impl ImageBackend,
    source: &Path,
    output_dir: &Path,
    filename_stem: &str,
    config: &ThumbnailConfig,
    ctx: &CacheContext<'_>,
) -> Result<(String, VariantStatus), ProcessError> {
    create_thumbnail_cached_with_suffix(
        backend,
        source,
        output_dir,
        filename_stem,
        "thumb",
        "",
        config,
        ctx,
    )
}

/// Create a thumbnail with cache awareness, using a custom filename suffix
/// and cache-variant tag so multiple thumbnail variants (e.g. regular +
/// full-index) can coexist per image.
///
/// `variant_tag` is mixed into the cache `params_hash`. Use `""` for the
/// legacy per-album thumbnail (matches the pre-variant hash exactly, so
/// existing caches are preserved), and a distinct string like
/// `"full-index"` for any other variant so its cache key never collides
/// with the regular thumbnail even when encode settings happen to match.
#[allow(clippy::too_many_arguments)]
fn create_thumbnail_cached_with_suffix(
    backend: &impl ImageBackend,
    source: &Path,
    output_dir: &Path,
    filename_stem: &str,
    suffix: &str,
    variant_tag: &str,
    config: &ThumbnailConfig,
    ctx: &CacheContext<'_>,
) -> Result<(String, VariantStatus), ProcessError> {
    let thumb_name = format!("{}-{}.avif", filename_stem, suffix);
    let relative_dir = output_dir
        .strip_prefix(ctx.cache_root)
        .unwrap()
        .to_str()
        .unwrap();
    let relative_path = format!("{}/{}", relative_dir, thumb_name);

    let sharpening_tuple = config.sharpening.map(|s| (s.sigma, s.threshold));
    let params_hash = cache::hash_thumbnail_variant_params(
        config.aspect,
        config.short_edge,
        config.quality.value(),
        sharpening_tuple,
        variant_tag,
    );

    let lookup = check_cache_and_copy(&relative_path, ctx.source_hash, &params_hash, ctx);
    match &lookup {
        CacheLookup::ExactHit => {
            ctx.stats.lock().unwrap().hit();
        }
        CacheLookup::Copied => {
            ctx.stats.lock().unwrap().copy();
        }
        CacheLookup::Miss => {
            let thumb_path = output_dir.join(&thumb_name);
            let params = crate::imaging::operations::plan_thumbnail(source, &thumb_path, config);
            backend.thumbnail(&params)?;
            ctx.cache.lock().unwrap().insert(
                relative_path.clone(),
                ctx.source_hash.to_string(),
                params_hash,
            );
            ctx.stats.lock().unwrap().miss();
        }
    }

    let status = VariantStatus::from(&lookup);
    Ok((relative_path, status))
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
    fn full_index_thumbnail_cache_does_not_collide_with_regular_thumbnail() {
        // Regression: when `[full_index]` and `[thumbnails]` share the same
        // ratio/size/quality/sharpening, the two thumbnail variants computed
        // identical `params_hash` values. CacheManifest::insert then evicted
        // the first entry when the second was inserted, making the cache
        // manifest lose track of one of the two files on disk.
        //
        // With the fix, the full-index thumbnail mixes a variant tag into its
        // hash so the two cache keys never collide, even when encode settings
        // match.
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let output_dir = tmp.path().join("output");

        let image_path = source_dir.join("test-album/001-test.jpg");
        create_dummy_source(&image_path);

        // full_index.generates = true at the site level, with defaults that
        // match [thumbnails] — the exact collision scenario.
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
                "in_nav": true,
                "config": {
                    "full_index": {"generates": true}
                }
            }],
            "config": {
                "full_index": {"generates": true}
            }
        }"##;
        let manifest_path = tmp.path().join("manifest.json");
        fs::write(&manifest_path, manifest).unwrap();

        let backend = MockBackend::with_dimensions(vec![Dimensions {
            width: 2000,
            height: 1500,
        }]);

        process_with_backend(
            &backend,
            &manifest_path,
            &source_dir,
            &output_dir,
            true,
            None,
        )
        .unwrap();

        // Both thumbnail files must be recorded in the cache manifest. If the
        // two variants share a params_hash, the second insert evicts the first.
        let cache_manifest = cache::CacheManifest::load(&output_dir);
        let paths: Vec<&String> = cache_manifest.entries.keys().collect();

        let has_regular = paths.iter().any(|p| p.ends_with("001-test-thumb.avif"));
        let has_fi = paths.iter().any(|p| p.ends_with("001-test-fi-thumb.avif"));

        assert!(
            has_regular,
            "regular thumbnail missing from cache manifest; entries: {:?}",
            paths
        );
        assert!(
            has_fi,
            "full-index thumbnail missing from cache manifest; entries: {:?}",
            paths
        );
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

    #[test]
    fn cache_hit_after_album_rename() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let output_dir = tmp.path().join("output");

        let image_path = source_dir.join("test-album/001-test.jpg");
        create_dummy_source(&image_path);

        // First run: album path is "test-album"
        let manifest_path =
            create_test_manifest_with_config(tmp.path(), r#"{"images": {"sizes": [800]}}"#);

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
        assert_eq!(stats1.hits, 0);

        // Create dummy output files (mock backend doesn't write real files)
        for entry in cache::CacheManifest::load(&output_dir).entries.keys() {
            let path = output_dir.join(entry);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "fake avif").unwrap();
        }

        // Second run: album renamed to "renamed-album", same source image
        let manifest2 = r##"{
            "navigation": [],
            "albums": [{
                "path": "renamed-album",
                "title": "Renamed Album",
                "description": null,
                "preview_image": "test-album/001-test.jpg",
                "images": [{
                    "number": 1,
                    "source_path": "test-album/001-test.jpg",
                    "filename": "001-test.jpg"
                }],
                "in_nav": true,
                "config": {"images": {"sizes": [800]}}
            }],
            "config": {}
        }"##;
        let manifest_path2 = tmp.path().join("manifest2.json");
        fs::write(&manifest_path2, manifest2).unwrap();

        let backend = MockBackend::with_dimensions(vec![Dimensions {
            width: 2000,
            height: 1500,
        }]);
        let result = process_with_backend(
            &backend,
            &manifest_path2,
            &source_dir,
            &output_dir,
            true,
            None,
        )
        .unwrap();

        // Should be copies (not re-encoded) since content is identical
        assert_eq!(result.cache_stats.copies, 2); // 1 resize + 1 thumb copied
        assert_eq!(result.cache_stats.misses, 0);
        assert_eq!(result.cache_stats.hits, 0);

        // Verify copied files exist at the new path
        assert!(output_dir.join("renamed-album/001-test-800.avif").exists());
        assert!(
            output_dir
                .join("renamed-album/001-test-thumb.avif")
                .exists()
        );

        // Verify stale entries were cleaned up
        let manifest = cache::CacheManifest::load(&output_dir);
        assert!(
            !manifest
                .entries
                .contains_key("test-album/001-test-800.avif")
        );
        assert!(
            !manifest
                .entries
                .contains_key("test-album/001-test-thumb.avif")
        );
        assert!(
            manifest
                .entries
                .contains_key("renamed-album/001-test-800.avif")
        );
        assert!(
            manifest
                .entries
                .contains_key("renamed-album/001-test-thumb.avif")
        );
    }

    // =========================================================================
    // Phase 2: canonical-image hash memoization
    // =========================================================================

    /// Build a manifest with two albums that each reference the same
    /// canonical image, plus the canonical_images array. Used to verify
    /// the per-canonical-content hash memo fires correctly.
    fn create_shared_canonical_manifest(tmp: &Path) -> PathBuf {
        let manifest = r##"{
            "navigation": [],
            "albums": [
                {
                    "path": "album-a",
                    "title": "Album A",
                    "description": null,
                    "preview_image": "album-a/001-shared.jpg",
                    "images": [{
                        "number": 1,
                        "source_path": "album-a/001-shared.jpg",
                        "filename": "001-shared.jpg",
                        "canonical_id": "sha256-aaa"
                    }],
                    "in_nav": true,
                    "config": {}
                },
                {
                    "path": "album-b",
                    "title": "Album B",
                    "description": null,
                    "preview_image": "album-b/001-shared.jpg",
                    "images": [{
                        "number": 1,
                        "source_path": "album-b/001-shared.jpg",
                        "filename": "001-shared.jpg",
                        "canonical_id": "sha256-aaa"
                    }],
                    "in_nav": true,
                    "config": {}
                }
            ],
            "config": {},
            "canonical_images": [
                {
                    "id": "sha256-aaa",
                    "source_path": "album-a/001-shared.jpg",
                    "aliases": ["album-b/001-shared.jpg"]
                }
            ]
        }"##;
        let manifest_path = tmp.join("manifest.json");
        fs::write(&manifest_path, manifest).unwrap();
        manifest_path
    }

    #[test]
    fn shared_canonical_image_hashed_exactly_once() {
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let output_dir = tmp.path().join("output");
        // Same bytes at two paths — scan would collapse these into one
        // canonical entry. Write the bytes twice to match what a real
        // scan would see on disk.
        create_dummy_source(&source_dir.join("album-a/001-shared.jpg"));
        create_dummy_source(&source_dir.join("album-b/001-shared.jpg"));

        let manifest_path = create_shared_canonical_manifest(tmp.path());
        // One dimension per image processed (two refs → two identify calls).
        let backend = MockBackend::with_dimensions(vec![
            Dimensions {
                width: 400,
                height: 300,
            },
            Dimensions {
                width: 400,
                height: 300,
            },
        ]);

        let result = process_with_backend(
            &backend,
            &manifest_path,
            &source_dir,
            &output_dir,
            false,
            None,
        )
        .unwrap();

        // Two refs, one canonical id → exactly one unique hash, one reuse.
        assert_eq!(
            result.source_hash_stats.unique, 1,
            "expected one hash_file call for the shared canonical image"
        );
        assert_eq!(
            result.source_hash_stats.reused, 1,
            "expected the second album's ref to reuse the memoized hash"
        );
        // Both albums still got processed.
        assert_eq!(result.manifest.albums.len(), 2);
        assert_eq!(result.manifest.albums[0].images.len(), 1);
        assert_eq!(result.manifest.albums[1].images.len(), 1);
    }

    #[test]
    fn legacy_manifest_without_canonical_fields_falls_back_to_path_key() {
        // Manifest has no canonical_id / canonical_images — simulates a
        // pre-Phase-1 scan. Hash memo should still dedupe by source_path
        // when two refs happen to have the same path (edge case) or
        // otherwise hash per-ref as before.
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let output_dir = tmp.path().join("output");
        create_dummy_source(&source_dir.join("test-album/001-test.jpg"));

        let manifest_path = create_test_manifest(tmp.path());
        let backend = MockBackend::with_dimensions(vec![Dimensions {
            width: 400,
            height: 300,
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

        // One ref, no canonical_id, fallback keys on source_path → one unique, zero reused.
        assert_eq!(result.source_hash_stats.unique, 1);
        assert_eq!(result.source_hash_stats.reused, 0);
    }

    #[test]
    fn different_canonical_ids_do_not_dedupe() {
        // Two refs with different canonical_ids (the scanner saw distinct
        // byte content) must hash twice — no false dedup even if paths
        // look similar.
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let output_dir = tmp.path().join("output");
        create_dummy_source(&source_dir.join("album-a/001-x.jpg"));
        create_dummy_source(&source_dir.join("album-b/001-x.jpg"));

        let manifest = r##"{
            "navigation": [],
            "albums": [
                {
                    "path": "album-a",
                    "title": "Album A",
                    "description": null,
                    "preview_image": "album-a/001-x.jpg",
                    "images": [{
                        "number": 1,
                        "source_path": "album-a/001-x.jpg",
                        "filename": "001-x.jpg",
                        "canonical_id": "sha256-aaa"
                    }],
                    "in_nav": true,
                    "config": {}
                },
                {
                    "path": "album-b",
                    "title": "Album B",
                    "description": null,
                    "preview_image": "album-b/001-x.jpg",
                    "images": [{
                        "number": 1,
                        "source_path": "album-b/001-x.jpg",
                        "filename": "001-x.jpg",
                        "canonical_id": "sha256-bbb"
                    }],
                    "in_nav": true,
                    "config": {}
                }
            ],
            "config": {},
            "canonical_images": [
                {"id": "sha256-aaa", "source_path": "album-a/001-x.jpg"},
                {"id": "sha256-bbb", "source_path": "album-b/001-x.jpg"}
            ]
        }"##;
        let manifest_path = tmp.path().join("manifest.json");
        fs::write(&manifest_path, manifest).unwrap();
        let backend = MockBackend::with_dimensions(vec![
            Dimensions {
                width: 400,
                height: 300,
            },
            Dimensions {
                width: 400,
                height: 300,
            },
        ]);

        let result = process_with_backend(
            &backend,
            &manifest_path,
            &source_dir,
            &output_dir,
            false,
            None,
        )
        .unwrap();

        assert_eq!(result.source_hash_stats.unique, 2);
        assert_eq!(result.source_hash_stats.reused, 0);
    }

    #[test]
    fn unresolved_canonical_id_falls_back_to_source_path_key() {
        // Defensive test: a manifest where `canonical_id` is set but the
        // id isn't present in `canonical_images` (inconsistent input).
        // Without the resolution guard, two genuinely distinct images
        // sharing a bad id would collide on memo_key and the second ref
        // would get the first ref's hash. With the guard, we fall back
        // to each ref's own `source_path` key so they hash independently
        // and get their correct, distinct hashes.
        let tmp = TempDir::new().unwrap();
        let source_dir = tmp.path().join("source");
        let output_dir = tmp.path().join("output");
        fs::create_dir_all(source_dir.join("album-a")).unwrap();
        fs::create_dir_all(source_dir.join("album-b")).unwrap();
        // Different bytes — correct behavior requires two hash calls
        // producing two distinct hashes.
        fs::write(source_dir.join("album-a/001-x.jpg"), b"alpha").unwrap();
        fs::write(source_dir.join("album-b/001-x.jpg"), b"beta").unwrap();

        let manifest = r##"{
            "navigation": [],
            "albums": [
                {
                    "path": "album-a",
                    "title": "Album A",
                    "description": null,
                    "preview_image": "album-a/001-x.jpg",
                    "images": [{
                        "number": 1,
                        "source_path": "album-a/001-x.jpg",
                        "filename": "001-x.jpg",
                        "canonical_id": "sha256-ghost"
                    }],
                    "in_nav": true,
                    "config": {}
                },
                {
                    "path": "album-b",
                    "title": "Album B",
                    "description": null,
                    "preview_image": "album-b/001-x.jpg",
                    "images": [{
                        "number": 1,
                        "source_path": "album-b/001-x.jpg",
                        "filename": "001-x.jpg",
                        "canonical_id": "sha256-ghost"
                    }],
                    "in_nav": true,
                    "config": {}
                }
            ],
            "config": {},
            "canonical_images": []
        }"##;
        let manifest_path = tmp.path().join("manifest.json");
        fs::write(&manifest_path, manifest).unwrap();
        let backend = MockBackend::with_dimensions(vec![
            Dimensions {
                width: 400,
                height: 300,
            },
            Dimensions {
                width: 400,
                height: 300,
            },
        ]);

        let result = process_with_backend(
            &backend,
            &manifest_path,
            &source_dir,
            &output_dir,
            false,
            None,
        )
        .unwrap();

        // Both refs hashed independently via source_path fallback despite
        // the shared (unresolved) canonical_id — no false dedup.
        assert_eq!(result.source_hash_stats.unique, 2);
        assert_eq!(result.source_hash_stats.reused, 0);
    }
}
