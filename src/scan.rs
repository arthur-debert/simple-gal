//! Filesystem scanning and manifest generation.
//!
//! Stage 1 of the Simple Gal build pipeline. Scans a directory tree to discover
//! albums and images, producing a structured manifest that subsequent stages consume.
//!
//! ## Directory Structure
//!
//! Simple Gal expects a specific directory layout:
//!
//! ```text
//! content/                         # Content root
//! ├── config.toml                  # Site configuration (optional)
//! ├── 040-about.md                 # Page (numbered = appears in nav)
//! ├── 050-github.md                # External link page (URL-only content)
//! ├── 010-Landscapes/              # Album (numbered = appears in nav)
//! │   ├── description.txt                 # Album description (optional)
//! │   ├── 001-dawn.jpg             # Preview image (lowest number)
//! │   ├── 002-sunset.jpg
//! │   └── 010-mountains.jpg
//! ├── 020-Travel/                  # Container directory (has subdirs)
//! │   ├── 010-Japan/               # Nested album
//! │   │   ├── 001-tokyo.jpg
//! │   │   └── 002-kyoto.jpg
//! │   └── 020-Italy/
//! │       └── 001-rome.jpg
//! ├── 030-Minimal/                 # Another album
//! │   └── 001-simple.jpg
//! └── wip-drafts/                  # Unnumbered = hidden from nav
//!     └── 001-draft.jpg
//! ```
//!
//! ## Naming Conventions
//!
//! - **Numbered directories** (`NNN-name`): Appear in navigation, sorted by number
//! - **Unnumbered directories**: Albums exist but are hidden from navigation
//! - **Numbered images** (`NNN-name.ext`): Sorted by number within album
//! - **Image #1**: Automatically becomes the album preview/thumbnail (falls back to first image)
//!
//! ## Output
//!
//! Produces a [`Manifest`] containing:
//! - Navigation tree (numbered directories only)
//! - All albums with their images
//! - Pages from markdown files (content pages and external links)
//! - Site configuration
//!
//! ## Validation
//!
//! The scanner enforces these rules:
//! - No mixed content (directories cannot contain both images and subdirectories)
//! - No duplicate image numbers within an album
//! - Every album must have at least one image

use crate::config::{self, SiteConfig};
use crate::metadata;
use crate::naming::parse_entry_name;
use crate::types::{NavItem, Page};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ScanError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),
    #[error("Directory contains both images and subdirectories: {0}")]
    MixedContent(PathBuf),
    #[error("Duplicate image number {0} in {1}")]
    DuplicateNumber(u32, PathBuf),
}

/// Manifest output from the scan stage
#[derive(Debug, Serialize)]
pub struct Manifest {
    pub navigation: Vec<NavItem>,
    pub albums: Vec<Album>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<Page>,
    pub config: SiteConfig,
}

/// Album with its images
#[derive(Debug, Serialize)]
pub struct Album {
    pub path: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub preview_image: String,
    pub images: Vec<Image>,
    pub in_nav: bool,
}

/// Image metadata
///
/// Image filenames follow `(<seq>-)?<title>.<ext>` format:
/// - `001-Museum.jpeg` → number=1, title=Some("Museum")
/// - `001.jpeg` → number=1, title=None
/// - `001-.jpeg` → number=1, title=None
/// - `Museum.jpg` → unnumbered, title=Some("Museum")
///
/// The sequence number controls sort order; the title (if present) is
/// displayed in the breadcrumb on the image detail page.
#[derive(Debug, Serialize)]
pub struct Image {
    pub number: u32,
    pub source_path: String,
    pub filename: String,
    /// URL-safe name from filename, dashes preserved (e.g., "L1020411" from "015-L1020411.jpg")
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Image description from sidecar `.txt` file (e.g., `001-photo.txt` for `001-photo.jpg`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

pub fn scan(root: &Path) -> Result<Manifest, ScanError> {
    let mut albums = Vec::new();
    let mut nav_items = Vec::new();

    scan_directory(root, root, &mut albums, &mut nav_items)?;

    // Strip number prefixes from output paths (used for URLs and output dirs).
    // Sorting has already happened with original paths, so this is safe.
    for album in &mut albums {
        album.path = slug_path(&album.path);
    }
    slugify_nav_paths(&mut nav_items);

    let pages = parse_pages(root)?;

    // Load site config (uses defaults if config.toml doesn't exist)
    let config = config::load_config(root)?;

    Ok(Manifest {
        navigation: nav_items,
        albums,
        pages,
        config,
    })
}

/// Convert a relative path to a slug path by stripping number prefixes from each component.
/// `"020-Travel/010-Japan"` → `"Travel/Japan"`
fn slug_path(rel_path: &str) -> String {
    rel_path
        .split('/')
        .map(|component| {
            let parsed = parse_entry_name(component);
            if parsed.name.is_empty() {
                component.to_string()
            } else {
                parsed.name
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Recursively strip number prefixes from all NavItem paths.
fn slugify_nav_paths(items: &mut [NavItem]) {
    for item in items.iter_mut() {
        item.path = slug_path(&item.path);
        slugify_nav_paths(&mut item.children);
    }
}

/// Parse all markdown files in the root directory into pages.
///
/// Each `.md` file becomes a page. Numbered files (`NNN-name.md`) appear in
/// navigation sorted by number; unnumbered files are generated but hidden.
/// If a file's only content is a URL, it becomes an external link in the nav.
fn parse_pages(root: &Path) -> Result<Vec<Page>, ScanError> {
    let mut md_files: Vec<PathBuf> = fs::read_dir(root)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .map(|e| e.eq_ignore_ascii_case("md"))
                    .unwrap_or(false)
        })
        .collect();

    md_files.sort();

    let mut pages = Vec::new();
    for md_path in &md_files {
        let stem = md_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        let parsed = parse_entry_name(&stem);
        let in_nav = parsed.number.is_some();
        let sort_key = parsed.number.unwrap_or(u32::MAX);
        let link_title = parsed.display_title;
        let slug = parsed.name;

        let content = fs::read_to_string(md_path)?;
        let trimmed = content.trim();

        // A page whose only content is a URL becomes an external link
        let is_link = !trimmed.contains('\n')
            && (trimmed.starts_with("http://") || trimmed.starts_with("https://"));

        let title = if is_link {
            link_title.clone()
        } else {
            content
                .lines()
                .find(|line| line.starts_with("# "))
                .map(|line| line.trim_start_matches("# ").trim().to_string())
                .unwrap_or_else(|| link_title.clone())
        };

        pages.push(Page {
            title,
            link_title,
            slug,
            body: content,
            in_nav,
            sort_key,
            is_link,
        });
    }

    pages.sort_by_key(|p| p.sort_key);
    Ok(pages)
}

fn scan_directory(
    path: &Path,
    root: &Path,
    albums: &mut Vec<Album>,
    nav_items: &mut Vec<NavItem>,
) -> Result<(), ScanError> {
    let entries = collect_entries(path)?;

    let images = entries.iter().filter(|e| is_image(e)).collect::<Vec<_>>();

    let subdirs = entries.iter().filter(|e| e.is_dir()).collect::<Vec<_>>();

    // Check for mixed content
    if !images.is_empty() && !subdirs.is_empty() {
        return Err(ScanError::MixedContent(path.to_path_buf()));
    }

    if !images.is_empty() {
        // This is an album
        let album = build_album(path, root, &images)?;
        let in_nav = album.in_nav;
        let title = album.title.clone();
        let album_path = album.path.clone();

        albums.push(album);

        // Add to nav if numbered
        if in_nav {
            nav_items.push(NavItem {
                title,
                path: album_path,
                children: vec![],
            });
        }
    } else if !subdirs.is_empty() {
        // This is a container directory
        let mut child_nav = Vec::new();

        // Sort subdirs by their number prefix
        let mut sorted_subdirs = subdirs.clone();
        sorted_subdirs.sort_by_key(|d| {
            let name = d.file_name().unwrap().to_string_lossy().to_string();
            (parse_entry_name(&name).number.unwrap_or(u32::MAX), name)
        });

        for subdir in sorted_subdirs {
            scan_directory(subdir, root, albums, &mut child_nav)?;
        }

        // If this directory is numbered, add it to nav with children
        if path != root {
            let dir_name = path.file_name().unwrap().to_string_lossy();
            let parsed = parse_entry_name(&dir_name);
            if parsed.number.is_some() {
                let rel_path = path.strip_prefix(root).unwrap();
                nav_items.push(NavItem {
                    title: parsed.display_title,
                    path: rel_path.to_string_lossy().to_string(),
                    children: child_nav,
                });
            } else {
                // Unnumbered container - its children still get added at this level
                nav_items.extend(child_nav);
            }
        } else {
            // Root directory - just extend nav_items with children
            nav_items.extend(child_nav);
        }
    }

    // Sort nav_items by their original directory number
    nav_items.sort_by_key(|item| {
        let dir_name = item.path.split('/').next_back().unwrap_or("");
        parse_entry_name(dir_name).number.unwrap_or(u32::MAX)
    });

    Ok(())
}

fn collect_entries(path: &Path) -> Result<Vec<PathBuf>, ScanError> {
    let mut entries: Vec<PathBuf> = fs::read_dir(path)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().unwrap().to_string_lossy();
            // Skip hidden files, description.txt, config.toml, and build artifacts
            !name.starts_with('.')
                && name != "description.txt"
                && name != "config.toml"
                && name != "processed"
                && name != "dist"
                && name != "manifest.json"
        })
        .collect();

    entries.sort();
    Ok(entries)
}

fn is_image(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    IMAGE_EXTENSIONS.contains(&ext.as_str())
}

fn build_album(path: &Path, root: &Path, images: &[&PathBuf]) -> Result<Album, ScanError> {
    let rel_path = path.strip_prefix(root).unwrap();
    let dir_name = path.file_name().unwrap().to_string_lossy();

    let parsed_dir = parse_entry_name(&dir_name);
    let in_nav = parsed_dir.number.is_some();
    let title = if in_nav {
        parsed_dir.display_title
    } else {
        dir_name.to_string()
    };

    // Parse image names and check for duplicates.
    // Store ParsedName alongside each image to avoid double-parsing.
    let mut numbered_images: BTreeMap<u32, (&PathBuf, crate::naming::ParsedName)> = BTreeMap::new();
    let mut unnumbered_counter = 0u32;
    for img in images {
        let filename = img.file_name().unwrap().to_string_lossy();
        let stem = Path::new(&*filename).file_stem().unwrap().to_string_lossy();
        let parsed = parse_entry_name(&stem);
        if let Some(num) = parsed.number {
            if numbered_images.contains_key(&num) {
                return Err(ScanError::DuplicateNumber(num, path.to_path_buf()));
            }
            numbered_images.insert(num, (img, parsed));
        } else {
            // Images without numbers get sorted to the end, preserving filename order
            let high_num = 1_000_000 + unnumbered_counter;
            unnumbered_counter += 1;
            numbered_images.insert(high_num, (img, parsed));
        }
    }

    // Find preview image (#1, or first image by sort order)
    let preview_image = numbered_images
        .iter()
        .find(|&(&num, _)| num == 1)
        .map(|(_, (path, _))| *path)
        .or_else(|| numbered_images.values().next().map(|(path, _)| *path))
        // Safe: build_album is only called with non-empty images
        .unwrap();

    let preview_rel = preview_image.strip_prefix(root).unwrap();

    // Build image list
    let images: Vec<Image> = numbered_images
        .iter()
        .map(|(&num, (img_path, parsed))| {
            let filename = img_path.file_name().unwrap().to_string_lossy().to_string();
            let title = if parsed.display_title.is_empty() {
                None
            } else {
                Some(parsed.display_title.clone())
            };
            let source = img_path.strip_prefix(root).unwrap();
            let description = metadata::read_sidecar(img_path);
            Image {
                number: num,
                source_path: source.to_string_lossy().to_string(),
                filename,
                slug: parsed.name.clone(),
                title,
                description,
            }
        })
        .collect();

    // Read description if present
    let info_path = path.join("description.txt");
    let description = if info_path.exists() {
        let content = fs::read_to_string(&info_path)?.trim().to_string();
        if content.is_empty() {
            None
        } else {
            Some(content)
        }
    } else {
        None
    };

    Ok(Album {
        path: rel_path.to_string_lossy().to_string(),
        title,
        description,
        preview_image: preview_rel.to_string_lossy().to_string(),
        images,
        in_nav,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_fixtures() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/content");

        // Copy fixture directory recursively
        copy_dir_recursive(&fixtures, tmp.path()).unwrap();
        tmp
    }

    fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());

            if src_path.is_dir() {
                fs::create_dir_all(&dst_path)?;
                copy_dir_recursive(&src_path, &dst_path)?;
            } else {
                fs::copy(&src_path, &dst_path)?;
            }
        }
        Ok(())
    }

    #[test]
    fn scan_finds_all_albums() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        // Should find 5 albums: Landscapes, Japan, Italy, Minimal, wip-drafts
        assert_eq!(manifest.albums.len(), 5);
    }

    #[test]
    fn numbered_albums_appear_in_nav() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        // Top level nav should have: Landscapes, Travel, Minimal (all numbered)
        assert_eq!(manifest.navigation.len(), 3);

        let titles: Vec<&str> = manifest
            .navigation
            .iter()
            .map(|n| n.title.as_str())
            .collect();
        assert!(titles.contains(&"Landscapes"));
        assert!(titles.contains(&"Travel"));
        assert!(titles.contains(&"Minimal"));
    }

    #[test]
    fn unnumbered_albums_hidden_from_nav() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        let wip = manifest
            .albums
            .iter()
            .find(|a| a.title == "wip-drafts")
            .unwrap();
        assert!(!wip.in_nav);
    }

    #[test]
    fn nested_albums_have_children_in_nav() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        let travel = manifest
            .navigation
            .iter()
            .find(|n| n.title == "Travel")
            .unwrap();
        assert_eq!(travel.children.len(), 2);

        let child_titles: Vec<&str> = travel.children.iter().map(|n| n.title.as_str()).collect();
        assert!(child_titles.contains(&"Japan"));
        assert!(child_titles.contains(&"Italy"));
    }

    #[test]
    fn images_sorted_by_number() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        let landscapes = manifest
            .albums
            .iter()
            .find(|a| a.title == "Landscapes")
            .unwrap();
        let numbers: Vec<u32> = landscapes.images.iter().map(|i| i.number).collect();

        assert_eq!(numbers, vec![1, 2, 10]);
    }

    #[test]
    fn image_title_extracted_in_scan() {
        let tmp = TempDir::new().unwrap();

        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-Dawn.jpg"), "fake image").unwrap();
        fs::write(album.join("002.jpg"), "fake image").unwrap();
        fs::write(album.join("003-My-Museum.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        let images = &manifest.albums[0].images;

        assert_eq!(images[0].title.as_deref(), Some("Dawn"));
        assert_eq!(images[1].title, None);
        assert_eq!(images[2].title.as_deref(), Some("My Museum"));
    }

    #[test]
    fn description_read_from_description_txt() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        let landscapes = manifest
            .albums
            .iter()
            .find(|a| a.title == "Landscapes")
            .unwrap();
        assert!(landscapes.description.is_some());
        assert!(
            landscapes
                .description
                .as_ref()
                .unwrap()
                .contains("landscape")
        );

        let minimal = manifest
            .albums
            .iter()
            .find(|a| a.title == "Minimal")
            .unwrap();
        assert!(minimal.description.is_none());
    }

    #[test]
    fn preview_image_is_001() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        let landscapes = manifest
            .albums
            .iter()
            .find(|a| a.title == "Landscapes")
            .unwrap();
        assert!(landscapes.preview_image.contains("001-dawn"));
    }

    #[test]
    fn mixed_content_is_error() {
        let tmp = TempDir::new().unwrap();

        // Create a directory with both images and subdirs
        let mixed = tmp.path().join("010-Mixed");
        fs::create_dir_all(&mixed).unwrap();
        fs::create_dir_all(mixed.join("subdir")).unwrap();

        // Create a placeholder image in mixed dir (scan only checks extension)
        fs::write(mixed.join("001-photo.jpg"), "fake image").unwrap();

        let result = scan(tmp.path());
        assert!(matches!(result, Err(ScanError::MixedContent(_))));
    }

    #[test]
    fn duplicate_number_is_error() {
        let tmp = TempDir::new().unwrap();

        let album = tmp.path().join("010-Album");
        fs::create_dir_all(&album).unwrap();

        // Create two images with the same number (scan only checks extension)
        fs::write(album.join("001-first.jpg"), "fake image").unwrap();
        fs::write(album.join("001-second.jpg"), "fake image").unwrap();

        let result = scan(tmp.path());
        assert!(matches!(result, Err(ScanError::DuplicateNumber(1, _))));
    }

    // =========================================================================
    // Page tests
    // =========================================================================

    #[test]
    fn pages_parsed_from_fixtures() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        // Fixtures have 040-about.md (numbered, in nav) and 050-github.md (link)
        assert!(manifest.pages.len() >= 2);

        let about = manifest.pages.iter().find(|p| p.slug == "about").unwrap();
        assert_eq!(about.title, "About This Gallery");
        assert_eq!(about.link_title, "about");
        assert!(about.body.contains("Simple Gal"));
        assert!(about.in_nav);
        assert!(!about.is_link);
    }

    #[test]
    fn page_link_title_from_filename() {
        let tmp = TempDir::new().unwrap();

        let md_path = tmp.path().join("010-who-am-i.md");
        fs::write(&md_path, "# My Title\n\nSome content.").unwrap();

        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();

        let page = manifest.pages.first().unwrap();
        assert_eq!(page.link_title, "who am i");
        assert_eq!(page.title, "My Title");
        assert_eq!(page.slug, "who-am-i");
        assert!(page.in_nav);
    }

    #[test]
    fn page_title_fallback_to_link_title() {
        let tmp = TempDir::new().unwrap();

        let md_path = tmp.path().join("010-about-me.md");
        fs::write(&md_path, "Just some content without a heading.").unwrap();

        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();

        let page = manifest.pages.first().unwrap();
        assert_eq!(page.title, "about me");
        assert_eq!(page.link_title, "about me");
    }

    #[test]
    fn no_pages_when_no_markdown() {
        let tmp = TempDir::new().unwrap();

        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        assert!(manifest.pages.is_empty());
    }

    #[test]
    fn unnumbered_page_hidden_from_nav() {
        let tmp = TempDir::new().unwrap();

        fs::write(tmp.path().join("notes.md"), "# Notes\n\nSome notes.").unwrap();

        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();

        let page = manifest.pages.first().unwrap();
        assert!(!page.in_nav);
        assert_eq!(page.slug, "notes");
    }

    #[test]
    fn link_page_detected() {
        let tmp = TempDir::new().unwrap();

        fs::write(
            tmp.path().join("050-github.md"),
            "https://github.com/example\n",
        )
        .unwrap();

        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();

        let page = manifest.pages.first().unwrap();
        assert!(page.is_link);
        assert!(page.in_nav);
        assert_eq!(page.link_title, "github");
        assert_eq!(page.slug, "github");
    }

    #[test]
    fn multiline_content_not_detected_as_link() {
        let tmp = TempDir::new().unwrap();

        fs::write(
            tmp.path().join("010-page.md"),
            "https://example.com\nsome other content",
        )
        .unwrap();

        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();

        let page = manifest.pages.first().unwrap();
        assert!(!page.is_link);
    }

    #[test]
    fn multiple_pages_sorted_by_number() {
        let tmp = TempDir::new().unwrap();

        fs::write(tmp.path().join("020-second.md"), "# Second").unwrap();
        fs::write(tmp.path().join("010-first.md"), "# First").unwrap();
        fs::write(tmp.path().join("030-third.md"), "# Third").unwrap();

        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();

        let titles: Vec<&str> = manifest.pages.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, vec!["First", "Second", "Third"]);
    }

    #[test]
    fn link_page_in_fixtures() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        let github = manifest.pages.iter().find(|p| p.slug == "github").unwrap();
        assert!(github.is_link);
        assert!(github.in_nav);
        assert!(github.body.trim().starts_with("https://"));
    }

    // =========================================================================
    // Config integration tests
    // =========================================================================

    #[test]
    fn config_loaded_from_fixtures() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        // Fixtures has a config.toml - verify it was loaded
        // (exact values depend on fixture content, just check it's not default)
        assert!(!manifest.config.colors.light.background.is_empty());
    }

    #[test]
    fn default_config_when_no_toml() {
        let tmp = TempDir::new().unwrap();

        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();

        // Should have default config values
        assert_eq!(manifest.config.colors.light.background, "#ffffff");
        assert_eq!(manifest.config.colors.dark.background, "#0a0a0a");
    }

    // =========================================================================
    // Album path and structure tests
    // =========================================================================

    #[test]
    fn album_paths_are_relative() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        for album in &manifest.albums {
            // Paths should not start with / or contain absolute paths
            assert!(!album.path.starts_with('/'));
            assert!(!album.path.contains(tmp.path().to_str().unwrap()));
        }
    }

    #[test]
    fn nested_album_path_includes_parent() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        let japan = manifest.albums.iter().find(|a| a.title == "Japan").unwrap();
        assert!(japan.path.contains("Travel"));
        assert!(japan.path.contains("Japan"));
        assert!(!japan.path.contains("020-"));
        assert!(!japan.path.contains("010-"));
    }

    #[test]
    fn image_source_paths_are_relative() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        for album in &manifest.albums {
            for image in &album.images {
                assert!(!image.source_path.starts_with('/'));
            }
        }
    }
}
