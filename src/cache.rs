//! Image processing cache for incremental builds.
//!
//! AVIF encoding is the bottleneck of the build pipeline — a single image
//! at three responsive sizes can take several seconds through rav1e. This
//! module lets the process stage skip encoding when the source image and
//! encoding parameters haven't changed since the last build.
//!
//! # Design
//!
//! The cache targets only the expensive encoding operations
//! ([`create_responsive_images`](crate::imaging::create_responsive_images) and
//! [`create_thumbnail`](crate::imaging::create_thumbnail)). Everything else
//! — dimension reads, IPTC metadata extraction, title/description resolution —
//! always runs. This means metadata changes (e.g. updating an IPTC title in
//! Lightroom) are picked up immediately without a cache bust.
//!
//! ## Cache keys
//!
//! The cache is **content-addressed**: lookups are by the combination of
//! `source_hash` and `params_hash`, not by output file path. This means
//! album renames, file renumbers, and slug changes do not invalidate the
//! cache — only actual image content or encoding parameter changes do.
//!
//! - **`source_hash`**: SHA-256 of the source file contents. Content-based
//!   rather than mtime-based so it survives `git checkout` (which resets
//!   modification times). Computed once per source file and shared across all
//!   its output variants.
//!
//! - **`params_hash`**: SHA-256 of the encoding parameters. For responsive
//!   variants this includes (target width, quality). For thumbnails it includes
//!   (aspect ratio, short edge, quality, sharpening). If any config value
//!   changes, the params hash changes and the image is re-encoded.
//!
//! A cache hit requires:
//! 1. An entry with matching `source_hash` and `params_hash` exists
//! 2. The previously-written output file still exists on disk
//!
//! When a hit is found but the output path has changed (e.g. album renamed),
//! the cached file is copied to the new location instead of re-encoding.
//!
//! ## Storage
//!
//! The cache manifest is a JSON file at `<output_dir>/.cache-manifest.json`.
//! It lives alongside the processed images so it travels with the output
//! directory when cached in CI (e.g. `actions/cache` on `dist/`).
//!
//! ## Bypassing the cache
//!
//! Pass `--no-cache` to the `build` or `process` command to force a full
//! rebuild. This loads an empty manifest, so every image is re-encoded. The
//! old output files are overwritten naturally.

use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// Name of the cache manifest file within the output directory.
const MANIFEST_FILENAME: &str = ".cache-manifest.json";

/// Version of the cache manifest format. Bump this to invalidate all
/// existing caches when the format or key computation changes.
const MANIFEST_VERSION: u32 = 1;

/// A single cached output file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct CacheEntry {
    pub source_hash: String,
    pub params_hash: String,
}

/// On-disk cache manifest mapping output paths to their cache entries.
///
/// Lookups go through a runtime `content_index` that maps
/// `"{source_hash}:{params_hash}"` to the stored output path, making
/// the cache resilient to album renames and file renumbering.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheManifest {
    pub version: u32,
    pub entries: HashMap<String, CacheEntry>,
    /// Runtime reverse index: `"{source_hash}:{params_hash}"` → output_path.
    /// Built at load time, maintained on insert. Never serialized.
    #[serde(skip)]
    content_index: HashMap<String, String>,
}

impl CacheManifest {
    /// Create an empty manifest (used for `--no-cache` or first build).
    pub fn empty() -> Self {
        Self {
            version: MANIFEST_VERSION,
            entries: HashMap::new(),
            content_index: HashMap::new(),
        }
    }

    /// Load from the output directory. Returns an empty manifest if the
    /// file doesn't exist or can't be parsed (version mismatch, corruption).
    pub fn load(output_dir: &Path) -> Self {
        let path = output_dir.join(MANIFEST_FILENAME);
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return Self::empty(),
        };
        let mut manifest: Self = match serde_json::from_str(&content) {
            Ok(m) => m,
            Err(_) => return Self::empty(),
        };
        if manifest.version != MANIFEST_VERSION {
            return Self::empty();
        }
        manifest.content_index = build_content_index(&manifest.entries);
        manifest
    }

    /// Save to the output directory.
    pub fn save(&self, output_dir: &Path) -> io::Result<()> {
        let path = output_dir.join(MANIFEST_FILENAME);
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)
    }

    /// Look up a cached output file by content hashes.
    ///
    /// Returns `Some(stored_output_path)` if an entry with matching
    /// `source_hash` and `params_hash` exists **and** the file is still
    /// on disk. The returned path may differ from the caller's expected
    /// output path (e.g. after an album rename); the caller is responsible
    /// for copying the file to the new location if needed.
    pub fn find_cached(
        &self,
        source_hash: &str,
        params_hash: &str,
        output_dir: &Path,
    ) -> Option<String> {
        let content_key = format!("{}:{}", source_hash, params_hash);
        let stored_path = self.content_index.get(&content_key)?;
        if output_dir.join(stored_path).exists() {
            Some(stored_path.clone())
        } else {
            None
        }
    }

    /// Record a cache entry for an output file.
    ///
    /// If an entry with the same content (source_hash + params_hash) already
    /// exists under a different output path, the old entry is removed to keep
    /// the manifest clean when images move (e.g. album rename).
    ///
    /// If the output path already has an entry for *different* content (e.g.
    /// image swap: file A moved to where B used to be), the old content's
    /// `content_index` entry is removed so stale lookups don't return a file
    /// whose content has been overwritten.
    pub fn insert(&mut self, output_path: String, source_hash: String, params_hash: String) {
        let content_key = format!("{}:{}", source_hash, params_hash);

        // Remove stale entry if content moved to a new path
        if let Some(old_path) = self.content_index.get(&content_key)
            && *old_path != output_path
        {
            self.entries.remove(old_path.as_str());
        }

        // If this output path previously held different content, invalidate
        // that content's lookup entry — the file on disk no longer matches.
        if let Some(displaced) = self.entries.get(&output_path) {
            let displaced_key = format!("{}:{}", displaced.source_hash, displaced.params_hash);
            if displaced_key != content_key {
                self.content_index.remove(&displaced_key);
            }
        }

        self.content_index.insert(content_key, output_path.clone());
        self.entries.insert(
            output_path,
            CacheEntry {
                source_hash,
                params_hash,
            },
        );
    }

    /// Remove all entries whose output path is not in `live_paths`, and
    /// delete the corresponding files from `output_dir`.
    ///
    /// Call this after a full build to clean up processed files for images
    /// that were deleted, renumbered, or belong to renamed/removed albums.
    pub fn prune(&mut self, live_paths: &HashSet<String>, output_dir: &Path) -> u32 {
        let stale: Vec<String> = self
            .entries
            .keys()
            .filter(|p| !live_paths.contains(p.as_str()))
            .cloned()
            .collect();

        let mut removed = 0u32;
        for path in &stale {
            if let Some(entry) = self.entries.remove(path) {
                let content_key = format!("{}:{}", entry.source_hash, entry.params_hash);
                self.content_index.remove(&content_key);
            }
            let file = output_dir.join(path);
            if file.exists() {
                let _ = std::fs::remove_file(&file);
            }
            removed += 1;
        }
        removed
    }
}

/// Build the content_index reverse map from the entries map.
fn build_content_index(entries: &HashMap<String, CacheEntry>) -> HashMap<String, String> {
    entries
        .iter()
        .map(|(output_path, entry)| {
            let content_key = format!("{}:{}", entry.source_hash, entry.params_hash);
            (content_key, output_path.clone())
        })
        .collect()
}

/// SHA-256 hash of a file's contents, returned as a hex string.
pub fn hash_file(path: &Path) -> io::Result<String> {
    let bytes = std::fs::read(path)?;
    let digest = Sha256::digest(&bytes);
    Ok(format!("{:x}", digest))
}

/// SHA-256 hash of encoding parameters for a responsive variant.
///
/// Inputs: target width and quality. If any of these change, the
/// previously cached output is invalid.
pub fn hash_responsive_params(target_width: u32, quality: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"responsive\0");
    hasher.update(target_width.to_le_bytes());
    hasher.update(quality.to_le_bytes());
    format!("{:x}", hasher.finalize())
}

/// SHA-256 hash of encoding parameters for a thumbnail.
///
/// Inputs: aspect ratio, short edge size, quality, and sharpening
/// settings. If any of these change, the thumbnail is re-generated.
pub fn hash_thumbnail_params(
    aspect: (u32, u32),
    short_edge: u32,
    quality: u32,
    sharpening: Option<(f32, i32)>,
) -> String {
    hash_thumbnail_variant_params(aspect, short_edge, quality, sharpening, "")
}

/// SHA-256 hash of encoding parameters for a named thumbnail variant.
///
/// When a single source image produces multiple thumbnails (e.g. the
/// per-album thumbnail plus a full-index thumbnail), each variant must
/// map to a distinct cache key even when ratio/size/quality/sharpening
/// happen to match — otherwise `CacheManifest::insert` treats the second
/// variant as a moved copy of the first and evicts its entry.
///
/// `variant` is a short discriminator (e.g. `"full-index"`). Passing an
/// empty string reproduces the legacy `hash_thumbnail_params` hash, so
/// existing per-album thumbnail caches are not invalidated.
pub fn hash_thumbnail_variant_params(
    aspect: (u32, u32),
    short_edge: u32,
    quality: u32,
    sharpening: Option<(f32, i32)>,
    variant: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"thumbnail\0");
    hasher.update(aspect.0.to_le_bytes());
    hasher.update(aspect.1.to_le_bytes());
    hasher.update(short_edge.to_le_bytes());
    hasher.update(quality.to_le_bytes());
    match sharpening {
        Some((sigma, threshold)) => {
            hasher.update(b"\x01");
            hasher.update(sigma.to_le_bytes());
            hasher.update(threshold.to_le_bytes());
        }
        None => {
            hasher.update(b"\x00");
        }
    }
    if !variant.is_empty() {
        hasher.update(b"\0variant\0");
        hasher.update(variant.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// Summary of cache performance for a build run.
#[derive(Debug, Default)]
pub struct CacheStats {
    pub hits: u32,
    pub copies: u32,
    pub misses: u32,
}

impl CacheStats {
    pub fn hit(&mut self) {
        self.hits += 1;
    }

    pub fn copy(&mut self) {
        self.copies += 1;
    }

    pub fn miss(&mut self) {
        self.misses += 1;
    }

    pub fn total(&self) -> u32 {
        self.hits + self.copies + self.misses
    }
}

impl fmt::Display for CacheStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.hits > 0 || self.copies > 0 {
            if self.copies > 0 {
                write!(
                    f,
                    "{} cached, {} copied, {} encoded ({} total)",
                    self.hits,
                    self.copies,
                    self.misses,
                    self.total()
                )
            } else {
                write!(
                    f,
                    "{} cached, {} encoded ({} total)",
                    self.hits,
                    self.misses,
                    self.total()
                )
            }
        } else {
            write!(f, "{} encoded", self.misses)
        }
    }
}

/// Resolve the cache manifest path for an output directory.
pub fn manifest_path(output_dir: &Path) -> PathBuf {
    output_dir.join(MANIFEST_FILENAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // =========================================================================
    // CacheManifest basics
    // =========================================================================

    #[test]
    fn empty_manifest_has_no_entries() {
        let m = CacheManifest::empty();
        assert_eq!(m.version, MANIFEST_VERSION);
        assert!(m.entries.is_empty());
        assert!(m.content_index.is_empty());
    }

    #[test]
    fn find_cached_hit() {
        let tmp = TempDir::new().unwrap();
        let mut m = CacheManifest::empty();
        m.insert("a/b.avif".into(), "src123".into(), "prm456".into());

        let out = tmp.path().join("a");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("b.avif"), "data").unwrap();

        assert_eq!(
            m.find_cached("src123", "prm456", tmp.path()),
            Some("a/b.avif".to_string())
        );
    }

    #[test]
    fn find_cached_miss_wrong_source_hash() {
        let tmp = TempDir::new().unwrap();
        let mut m = CacheManifest::empty();
        m.insert("out.avif".into(), "hash_a".into(), "params".into());
        fs::write(tmp.path().join("out.avif"), "data").unwrap();

        assert_eq!(m.find_cached("hash_b", "params", tmp.path()), None);
    }

    #[test]
    fn find_cached_miss_wrong_params_hash() {
        let tmp = TempDir::new().unwrap();
        let mut m = CacheManifest::empty();
        m.insert("out.avif".into(), "hash".into(), "params_a".into());
        fs::write(tmp.path().join("out.avif"), "data").unwrap();

        assert_eq!(m.find_cached("hash", "params_b", tmp.path()), None);
    }

    #[test]
    fn find_cached_miss_file_deleted() {
        let mut m = CacheManifest::empty();
        m.insert("gone.avif".into(), "h".into(), "p".into());
        let tmp = TempDir::new().unwrap();
        // File doesn't exist
        assert_eq!(m.find_cached("h", "p", tmp.path()), None);
    }

    #[test]
    fn find_cached_miss_no_entry() {
        let m = CacheManifest::empty();
        let tmp = TempDir::new().unwrap();
        assert_eq!(m.find_cached("h", "p", tmp.path()), None);
    }

    #[test]
    fn find_cached_returns_old_path_after_content_match() {
        let tmp = TempDir::new().unwrap();
        let mut m = CacheManifest::empty();
        m.insert(
            "old-album/01-800.avif".into(),
            "srchash".into(),
            "prmhash".into(),
        );

        let old_dir = tmp.path().join("old-album");
        fs::create_dir_all(&old_dir).unwrap();
        fs::write(old_dir.join("01-800.avif"), "avif data").unwrap();

        let result = m.find_cached("srchash", "prmhash", tmp.path());
        assert_eq!(result, Some("old-album/01-800.avif".to_string()));
    }

    #[test]
    fn insert_removes_stale_entry_on_path_change() {
        let mut m = CacheManifest::empty();
        m.insert("old-album/img-800.avif".into(), "src".into(), "prm".into());
        assert!(m.entries.contains_key("old-album/img-800.avif"));

        // Insert same content under new path
        m.insert("new-album/img-800.avif".into(), "src".into(), "prm".into());

        assert!(!m.entries.contains_key("old-album/img-800.avif"));
        assert!(m.entries.contains_key("new-album/img-800.avif"));
    }

    #[test]
    fn insert_invalidates_displaced_content_index() {
        let mut m = CacheManifest::empty();
        // Path "album/309-800.avif" holds content A
        m.insert(
            "album/309-800.avif".into(),
            "hash_A".into(),
            "params".into(),
        );
        assert_eq!(
            m.content_index.get("hash_A:params"),
            Some(&"album/309-800.avif".to_string())
        );

        // Now content B overwrites that path (image swap)
        m.insert(
            "album/309-800.avif".into(),
            "hash_B".into(),
            "params".into(),
        );

        // hash_A's content_index entry should be gone (file overwritten)
        assert_eq!(m.content_index.get("hash_A:params"), None);
        // hash_B points to the path
        assert_eq!(
            m.content_index.get("hash_B:params"),
            Some(&"album/309-800.avif".to_string())
        );
    }

    #[test]
    fn prune_removes_stale_entries_and_files() {
        let tmp = TempDir::new().unwrap();
        let mut m = CacheManifest::empty();
        m.insert("album/live.avif".into(), "s1".into(), "p1".into());
        m.insert("album/stale.avif".into(), "s2".into(), "p2".into());

        // Create both files on disk
        let dir = tmp.path().join("album");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("live.avif"), "data").unwrap();
        fs::write(dir.join("stale.avif"), "data").unwrap();

        let mut live = HashSet::new();
        live.insert("album/live.avif".to_string());
        let removed = m.prune(&live, tmp.path());

        assert_eq!(removed, 1);
        assert!(m.entries.contains_key("album/live.avif"));
        assert!(!m.entries.contains_key("album/stale.avif"));
        assert!(dir.join("live.avif").exists());
        assert!(!dir.join("stale.avif").exists());
    }

    #[test]
    fn content_index_rebuilt_on_load() {
        let tmp = TempDir::new().unwrap();
        let mut m = CacheManifest::empty();
        m.insert("a/x.avif".into(), "s1".into(), "p1".into());
        m.insert("b/y.avif".into(), "s2".into(), "p2".into());
        m.save(tmp.path()).unwrap();

        let loaded = CacheManifest::load(tmp.path());
        assert_eq!(
            loaded.find_cached("s1", "p1", tmp.path()),
            None // files don't exist, but index was built
        );
        assert_eq!(
            loaded.content_index.get("s1:p1"),
            Some(&"a/x.avif".to_string())
        );
        assert_eq!(
            loaded.content_index.get("s2:p2"),
            Some(&"b/y.avif".to_string())
        );
    }

    // =========================================================================
    // Save / Load roundtrip
    // =========================================================================

    #[test]
    fn save_and_load_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let mut m = CacheManifest::empty();
        m.insert("x.avif".into(), "s1".into(), "p1".into());
        m.insert("y.avif".into(), "s2".into(), "p2".into());

        m.save(tmp.path()).unwrap();
        let loaded = CacheManifest::load(tmp.path());

        assert_eq!(loaded.version, MANIFEST_VERSION);
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(
            loaded.entries["x.avif"],
            CacheEntry {
                source_hash: "s1".into(),
                params_hash: "p1".into()
            }
        );
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let m = CacheManifest::load(tmp.path());
        assert!(m.entries.is_empty());
    }

    #[test]
    fn load_corrupt_json_returns_empty() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join(MANIFEST_FILENAME), "not json").unwrap();
        let m = CacheManifest::load(tmp.path());
        assert!(m.entries.is_empty());
    }

    #[test]
    fn load_wrong_version_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let json = format!(
            r#"{{"version": {}, "entries": {{"a": {{"source_hash":"h","params_hash":"p"}}}}}}"#,
            MANIFEST_VERSION + 1
        );
        fs::write(tmp.path().join(MANIFEST_FILENAME), json).unwrap();
        let m = CacheManifest::load(tmp.path());
        assert!(m.entries.is_empty());
    }

    // =========================================================================
    // Hash functions
    // =========================================================================

    #[test]
    fn hash_file_deterministic() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.bin");
        fs::write(&path, b"hello world").unwrap();

        let h1 = hash_file(&path).unwrap();
        let h2 = hash_file(&path).unwrap();
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex is 64 chars
    }

    #[test]
    fn hash_file_changes_with_content() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.bin");

        fs::write(&path, b"version 1").unwrap();
        let h1 = hash_file(&path).unwrap();

        fs::write(&path, b"version 2").unwrap();
        let h2 = hash_file(&path).unwrap();

        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_responsive_params_deterministic() {
        let h1 = hash_responsive_params(1400, 90);
        let h2 = hash_responsive_params(1400, 90);
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_responsive_params_varies_with_width() {
        assert_ne!(
            hash_responsive_params(800, 90),
            hash_responsive_params(1400, 90)
        );
    }

    #[test]
    fn hash_responsive_params_varies_with_quality() {
        assert_ne!(
            hash_responsive_params(800, 85),
            hash_responsive_params(800, 90)
        );
    }

    #[test]
    fn hash_thumbnail_params_deterministic() {
        let h1 = hash_thumbnail_params((4, 5), 400, 90, Some((0.5, 0)));
        let h2 = hash_thumbnail_params((4, 5), 400, 90, Some((0.5, 0)));
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_thumbnail_params_varies_with_aspect() {
        assert_ne!(
            hash_thumbnail_params((4, 5), 400, 90, None),
            hash_thumbnail_params((16, 9), 400, 90, None)
        );
    }

    #[test]
    fn hash_thumbnail_params_varies_with_sharpening() {
        assert_ne!(
            hash_thumbnail_params((4, 5), 400, 90, Some((0.5, 0))),
            hash_thumbnail_params((4, 5), 400, 90, None)
        );
    }

    #[test]
    fn hash_thumbnail_variant_empty_tag_matches_legacy() {
        // Passing an empty variant tag must produce the exact same hash as
        // hash_thumbnail_params so existing per-album thumbnail caches are
        // not silently invalidated by the new variant-aware call.
        let legacy = hash_thumbnail_params((4, 5), 400, 90, Some((0.5, 0)));
        let empty_tag = hash_thumbnail_variant_params((4, 5), 400, 90, Some((0.5, 0)), "");
        assert_eq!(legacy, empty_tag);
    }

    #[test]
    fn hash_thumbnail_variant_differs_from_untagged_even_when_settings_match() {
        // Regression: when [full_index] and [thumbnails] use matching
        // ratio/size/quality/sharpening, the two variants must still map
        // to distinct cache keys so one doesn't evict the other on insert.
        let regular = hash_thumbnail_params((4, 5), 400, 90, Some((0.5, 0)));
        let full_index =
            hash_thumbnail_variant_params((4, 5), 400, 90, Some((0.5, 0)), "full-index");
        assert_ne!(regular, full_index);
    }

    #[test]
    fn hash_thumbnail_variant_different_tags_differ() {
        let a = hash_thumbnail_variant_params((4, 5), 400, 90, None, "full-index");
        let b = hash_thumbnail_variant_params((4, 5), 400, 90, None, "print-sheet");
        assert_ne!(a, b);
    }

    #[test]
    fn insert_does_not_evict_regular_thumbnail_when_variant_tag_differs() {
        // Full regression at the cache manifest level: inserting a regular
        // thumbnail and then a variant thumbnail with matching encode
        // settings must leave BOTH entries in the manifest.
        let mut m = CacheManifest::empty();
        let regular_hash = hash_thumbnail_variant_params((4, 5), 400, 90, None, "");
        let fi_hash = hash_thumbnail_variant_params((4, 5), 400, 90, None, "full-index");

        m.insert("a/001-test-thumb.avif".into(), "src".into(), regular_hash);
        m.insert("a/001-test-fi-thumb.avif".into(), "src".into(), fi_hash);

        assert!(m.entries.contains_key("a/001-test-thumb.avif"));
        assert!(m.entries.contains_key("a/001-test-fi-thumb.avif"));
    }

    // =========================================================================
    // CacheStats
    // =========================================================================

    #[test]
    fn cache_stats_display_with_hits() {
        let mut s = CacheStats::default();
        s.hits = 5;
        s.misses = 2;
        assert_eq!(format!("{}", s), "5 cached, 2 encoded (7 total)");
    }

    #[test]
    fn cache_stats_display_with_copies() {
        let mut s = CacheStats::default();
        s.hits = 3;
        s.copies = 2;
        s.misses = 1;
        assert_eq!(format!("{}", s), "3 cached, 2 copied, 1 encoded (6 total)");
    }

    #[test]
    fn cache_stats_display_no_hits() {
        let mut s = CacheStats::default();
        s.misses = 3;
        assert_eq!(format!("{}", s), "3 encoded");
    }
}
