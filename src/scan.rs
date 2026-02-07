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

/// Album with its images and resolved configuration.
#[derive(Debug, Serialize)]
pub struct Album {
    pub path: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub preview_image: String,
    pub images: Vec<Image>,
    pub in_nav: bool,
    /// Resolved config for this album (stock → root → group → gallery chain).
    pub config: SiteConfig,
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

    // Build the config chain: stock defaults → root config.toml
    let base = SiteConfig::default();
    let root_partial = config::load_partial_config(root)?;
    let root_config = match root_partial {
        Some(partial) => base.merge(partial),
        None => base,
    };

    scan_directory(root, root, &mut albums, &mut nav_items, &root_config)?;

    // Strip number prefixes from output paths (used for URLs and output dirs).
    // Sorting has already happened with original paths, so this is safe.
    for album in &mut albums {
        album.path = slug_path(&album.path);
    }
    slugify_nav_paths(&mut nav_items);

    let pages = parse_pages(root)?;

    // Root-level resolved config for CSS generation
    let config = root_config;

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
    inherited_config: &SiteConfig,
) -> Result<(), ScanError> {
    let entries = collect_entries(path)?;

    let images = entries.iter().filter(|e| is_image(e)).collect::<Vec<_>>();

    let subdirs = entries.iter().filter(|e| e.is_dir()).collect::<Vec<_>>();

    // Check for mixed content
    if !images.is_empty() && !subdirs.is_empty() {
        return Err(ScanError::MixedContent(path.to_path_buf()));
    }

    // Merge any local config.toml onto the inherited config (skip root — already handled)
    let effective_config = if path != root {
        match config::load_partial_config(path)? {
            Some(partial) => inherited_config.clone().merge(partial),
            None => inherited_config.clone(),
        }
    } else {
        inherited_config.clone()
    };

    if !images.is_empty() {
        // This is an album
        effective_config.validate()?;
        let album = build_album(path, root, &images, effective_config)?;
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
            scan_directory(subdir, root, albums, &mut child_nav, &effective_config)?;
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
            // Skip hidden files, description files, config.toml, and build artifacts
            !name.starts_with('.')
                && name != "description.txt"
                && name != "description.md"
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

/// Read an album description from `description.md` or `description.txt`.
///
/// - `description.md` takes priority and is rendered as markdown HTML.
/// - `description.txt` is converted to HTML with smart paragraph handling
///   and URL linkification.
/// - Returns `None` if neither file exists or contents are empty.
fn read_album_description(album_dir: &Path) -> Result<Option<String>, ScanError> {
    let md_path = album_dir.join("description.md");
    if md_path.exists() {
        let content = fs::read_to_string(&md_path)?.trim().to_string();
        if content.is_empty() {
            return Ok(None);
        }
        let parser = pulldown_cmark::Parser::new(&content);
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, parser);
        return Ok(Some(html));
    }

    let txt_path = album_dir.join("description.txt");
    if txt_path.exists() {
        let content = fs::read_to_string(&txt_path)?.trim().to_string();
        if content.is_empty() {
            return Ok(None);
        }
        return Ok(Some(plain_text_to_html(&content)));
    }

    Ok(None)
}

/// Convert plain text to HTML with smart paragraph detection and URL linkification.
///
/// - Double newlines (`\n\n`) split text into `<p>` elements.
/// - URLs starting with `http://` or `https://` are wrapped in `<a>` tags.
fn plain_text_to_html(text: &str) -> String {
    let paragraphs: Vec<&str> = text.split("\n\n").collect();
    paragraphs
        .iter()
        .map(|p| {
            let escaped = linkify_urls(&html_escape(p.trim()));
            format!("<p>{}</p>", escaped)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Escape HTML special characters.
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Find URLs in text and wrap them in anchor tags.
fn linkify_urls(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;

    while let Some(start) = remaining
        .find("https://")
        .or_else(|| remaining.find("http://"))
    {
        result.push_str(&remaining[..start]);
        let url_text = &remaining[start..];
        let end = url_text
            .find(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"')
            .unwrap_or(url_text.len());
        let url = &url_text[..end];
        result.push_str(&format!(r#"<a href="{url}">{url}</a>"#));
        remaining = &url_text[end..];
    }
    result.push_str(remaining);
    result
}

fn build_album(
    path: &Path,
    root: &Path,
    images: &[&PathBuf],
    config: SiteConfig,
) -> Result<Album, ScanError> {
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

    // Read description: description.md takes priority over description.txt
    let description = read_album_description(path)?;

    Ok(Album {
        path: rel_path.to_string_lossy().to_string(),
        title,
        description,
        preview_image: preview_rel.to_string_lossy().to_string(),
        images,
        in_nav,
        config,
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
        let desc = landscapes.description.as_ref().unwrap();
        // Should be wrapped in <p> tags (plain text → HTML conversion)
        assert!(desc.contains("<p>"));
        assert!(desc.contains("landscape"));

        let minimal = manifest
            .albums
            .iter()
            .find(|a| a.title == "Minimal")
            .unwrap();
        assert!(minimal.description.is_none());
    }

    #[test]
    fn description_md_takes_priority_over_txt() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();
        fs::write(album.join("description.txt"), "Text version").unwrap();
        fs::write(album.join("description.md"), "**Markdown** version").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        let desc = manifest.albums[0].description.as_ref().unwrap();
        assert!(desc.contains("<strong>Markdown</strong>"));
        assert!(!desc.contains("Text version"));
    }

    #[test]
    fn description_txt_converts_paragraphs() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();
        fs::write(
            album.join("description.txt"),
            "First paragraph.\n\nSecond paragraph.",
        )
        .unwrap();

        let manifest = scan(tmp.path()).unwrap();
        let desc = manifest.albums[0].description.as_ref().unwrap();
        assert!(desc.contains("<p>First paragraph.</p>"));
        assert!(desc.contains("<p>Second paragraph.</p>"));
    }

    #[test]
    fn description_txt_linkifies_urls() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();
        fs::write(
            album.join("description.txt"),
            "Visit https://example.com for more.",
        )
        .unwrap();

        let manifest = scan(tmp.path()).unwrap();
        let desc = manifest.albums[0].description.as_ref().unwrap();
        assert!(desc.contains(r#"<a href="https://example.com">https://example.com</a>"#));
    }

    #[test]
    fn description_md_renders_markdown() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();
        fs::write(
            album.join("description.md"),
            "# Title\n\nSome *italic* text.",
        )
        .unwrap();

        let manifest = scan(tmp.path()).unwrap();
        let desc = manifest.albums[0].description.as_ref().unwrap();
        assert!(desc.contains("<h1>Title</h1>"));
        assert!(desc.contains("<em>italic</em>"));
    }

    #[test]
    fn description_empty_file_returns_none() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();
        fs::write(album.join("description.txt"), "   \n  ").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        assert!(manifest.albums[0].description.is_none());
    }

    #[test]
    fn description_txt_escapes_html() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();
        fs::write(
            album.join("description.txt"),
            "<script>alert('xss')</script>",
        )
        .unwrap();

        let manifest = scan(tmp.path()).unwrap();
        let desc = manifest.albums[0].description.as_ref().unwrap();
        assert!(!desc.contains("<script>"));
        assert!(desc.contains("&lt;script&gt;"));
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

    // =========================================================================
    // plain_text_to_html and linkify_urls unit tests
    // =========================================================================

    #[test]
    fn plain_text_single_paragraph() {
        assert_eq!(plain_text_to_html("Hello world"), "<p>Hello world</p>");
    }

    #[test]
    fn plain_text_multiple_paragraphs() {
        assert_eq!(
            plain_text_to_html("First.\n\nSecond."),
            "<p>First.</p>\n<p>Second.</p>"
        );
    }

    #[test]
    fn linkify_urls_https() {
        assert_eq!(
            linkify_urls("Visit https://example.com today"),
            r#"Visit <a href="https://example.com">https://example.com</a> today"#
        );
    }

    #[test]
    fn linkify_urls_http() {
        assert_eq!(
            linkify_urls("See http://example.com here"),
            r#"See <a href="http://example.com">http://example.com</a> here"#
        );
    }

    #[test]
    fn linkify_urls_no_urls() {
        assert_eq!(linkify_urls("No links here"), "No links here");
    }

    #[test]
    fn linkify_urls_at_end_of_text() {
        assert_eq!(
            linkify_urls("Check https://example.com"),
            r#"Check <a href="https://example.com">https://example.com</a>"#
        );
    }

    // =========================================================================
    // Per-gallery config chain tests
    // =========================================================================

    #[test]
    fn album_gets_default_config_when_no_configs() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        assert_eq!(manifest.albums[0].config.images.quality, 90);
        assert_eq!(manifest.albums[0].config.thumbnails.aspect_ratio, [4, 5]);
    }

    #[test]
    fn album_inherits_root_config() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("config.toml"), "[images]\nquality = 85\n").unwrap();

        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        assert_eq!(manifest.albums[0].config.images.quality, 85);
        // Other defaults preserved
        assert_eq!(
            manifest.albums[0].config.images.sizes,
            vec![800, 1400, 2080]
        );
    }

    #[test]
    fn album_config_overrides_root() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("config.toml"), "[images]\nquality = 85\n").unwrap();

        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();
        fs::write(album.join("config.toml"), "[images]\nquality = 70\n").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        assert_eq!(manifest.albums[0].config.images.quality, 70);
        // Root config still at 85
        assert_eq!(manifest.config.images.quality, 85);
    }

    #[test]
    fn nested_config_chain_three_levels() {
        let tmp = TempDir::new().unwrap();

        // Root config: quality = 85
        fs::write(tmp.path().join("config.toml"), "[images]\nquality = 85\n").unwrap();

        // Group: Travel with aspect_ratio override
        let travel = tmp.path().join("020-Travel");
        fs::create_dir_all(&travel).unwrap();
        fs::write(
            travel.join("config.toml"),
            "[thumbnails]\naspect_ratio = [1, 1]\n",
        )
        .unwrap();

        // Gallery: Japan with quality override
        let japan = travel.join("010-Japan");
        fs::create_dir_all(&japan).unwrap();
        fs::write(japan.join("001-tokyo.jpg"), "fake image").unwrap();
        fs::write(japan.join("config.toml"), "[images]\nquality = 70\n").unwrap();

        // Gallery: Italy with no config
        let italy = travel.join("020-Italy");
        fs::create_dir_all(&italy).unwrap();
        fs::write(italy.join("001-rome.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();

        let japan_album = manifest.albums.iter().find(|a| a.title == "Japan").unwrap();
        // Japan: quality from its own config (70), aspect from group (1:1), sizes from stock
        assert_eq!(japan_album.config.images.quality, 70);
        assert_eq!(japan_album.config.thumbnails.aspect_ratio, [1, 1]);
        assert_eq!(japan_album.config.images.sizes, vec![800, 1400, 2080]);

        let italy_album = manifest.albums.iter().find(|a| a.title == "Italy").unwrap();
        // Italy: quality from root (85), aspect from group (1:1)
        assert_eq!(italy_album.config.images.quality, 85);
        assert_eq!(italy_album.config.thumbnails.aspect_ratio, [1, 1]);
    }

    #[test]
    fn album_config_unknown_key_rejected() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();
        fs::write(album.join("config.toml"), "[images]\nqualty = 90\n").unwrap();

        let result = scan(tmp.path());
        assert!(result.is_err());
    }
}
