//! Shared test utilities for the simple-gal test suite.
//!
//! Provides lookup helpers, bulk extractors, and navigation tree assertions
//! that work with scan-phase data structures (`Manifest`, `Album`, `Image`).
//!
//! # Usage
//!
//! ```rust
//! use crate::test_helpers::*;
//!
//! let tmp = setup_fixtures();
//! let manifest = scan(tmp.path()).unwrap();
//!
//! let album = find_album(&manifest, "Landscapes");
//! let image = find_image(album, "dawn");
//! assert_eq!(image.title.as_deref(), Some("dawn"));
//!
//! assert_nav_shape(&manifest, &[
//!     ("Landscapes", &[]),
//!     ("Travel", &["Japan", "Italy"]),
//!     ("Minimal", &[]),
//! ]);
//! ```

use std::path::Path;
use tempfile::TempDir;

use crate::scan::{Album, Image, Manifest};
use crate::types::Page;

// =========================================================================
// Fixture setup
// =========================================================================

/// Copy `fixtures/content/` to a temp directory and return it.
///
/// Tests get an isolated copy they can mutate without affecting other tests
/// or the source fixtures.
pub fn setup_fixtures() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/content");
    copy_dir_recursive(&fixtures, tmp.path()).unwrap();
    tmp
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path)?;
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

// =========================================================================
// Manifest lookups — panics with a clear message on miss
// =========================================================================

/// Find an album by title. Panics if not found.
pub fn find_album<'a>(manifest: &'a Manifest, title: &str) -> &'a Album {
    manifest
        .albums
        .iter()
        .find(|a| a.title == title)
        .unwrap_or_else(|| {
            let titles: Vec<&str> = manifest.albums.iter().map(|a| a.title.as_str()).collect();
            panic!("album '{title}' not found. Available: {titles:?}")
        })
}

/// Find an image by slug within an album. Panics if not found.
pub fn find_image<'a>(album: &'a Album, slug: &str) -> &'a Image {
    album
        .images
        .iter()
        .find(|i| i.slug == slug)
        .unwrap_or_else(|| {
            let slugs: Vec<&str> = album.images.iter().map(|i| i.slug.as_str()).collect();
            panic!(
                "image '{slug}' not found in album '{}'. Available: {slugs:?}",
                album.title
            )
        })
}

/// Find a page by slug. Panics if not found.
pub fn find_page<'a>(manifest: &'a Manifest, slug: &str) -> &'a Page {
    manifest
        .pages
        .iter()
        .find(|p| p.slug == slug)
        .unwrap_or_else(|| {
            let slugs: Vec<&str> = manifest.pages.iter().map(|p| p.slug.as_str()).collect();
            panic!("page '{slug}' not found. Available: {slugs:?}")
        })
}

// =========================================================================
// Bulk extractors
// =========================================================================

/// All album titles in manifest order.
pub fn album_titles(manifest: &Manifest) -> Vec<&str> {
    manifest.albums.iter().map(|a| a.title.as_str()).collect()
}

/// All image titles in album order.
pub fn image_titles(album: &Album) -> Vec<Option<&str>> {
    album.images.iter().map(|i| i.title.as_deref()).collect()
}

/// All image descriptions in album order.
pub fn image_descriptions(album: &Album) -> Vec<Option<&str>> {
    album
        .images
        .iter()
        .map(|i| i.description.as_deref())
        .collect()
}

// =========================================================================
// Navigation helpers
// =========================================================================

/// Top-level navigation titles in order.
pub fn nav_titles(manifest: &Manifest) -> Vec<&str> {
    manifest
        .navigation
        .iter()
        .map(|n| n.title.as_str())
        .collect()
}

/// Child titles under a given nav parent. Panics if parent not found.
pub fn nav_children_titles<'a>(manifest: &'a Manifest, parent_title: &str) -> Vec<&'a str> {
    manifest
        .navigation
        .iter()
        .find(|n| n.title == parent_title)
        .map(|n| n.children.iter().map(|c| c.title.as_str()).collect())
        .unwrap_or_else(|| {
            let titles = nav_titles(manifest);
            panic!("nav item '{parent_title}' not found. Available: {titles:?}")
        })
}

/// Assert that the full navigation tree matches an expected shape.
///
/// Each entry is `(title, children)`. Use `&[]` for leaf nodes.
///
/// ```rust
/// assert_nav_shape(&manifest, &[
///     ("Landscapes", &[]),
///     ("Travel", &["Japan", "Italy"]),
///     ("Minimal", &[]),
/// ]);
/// ```
pub fn assert_nav_shape(manifest: &Manifest, expected: &[(&str, &[&str])]) {
    let actual: Vec<&str> = nav_titles(manifest);
    let expected_titles: Vec<&str> = expected.iter().map(|(t, _)| *t).collect();
    assert_eq!(actual, expected_titles, "nav top-level titles mismatch");

    for (title, children) in expected {
        let actual_children = nav_children_titles(manifest, title);
        assert_eq!(
            actual_children,
            children.to_vec(),
            "nav children of '{title}' mismatch"
        );
    }
}
