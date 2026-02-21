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
//! - **Thumb images** (`NNN-thumb.ext` or `NNN-thumb-Title.ext`): Designated album thumbnail
//! - **Image #1**: Fallback album preview/thumbnail when no thumb image exists
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
    #[error("Multiple thumb-designated images in {0}")]
    DuplicateThumb(PathBuf),
}

/// Manifest output from the scan stage
#[derive(Debug, Serialize)]
pub struct Manifest {
    pub navigation: Vec<NavItem>,
    pub albums: Vec<Album>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<Page>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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
    /// Supporting files found in the album directory (e.g. config.toml, description.md).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub support_files: Vec<String>,
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

    scan_directory(
        root,
        root,
        &mut albums,
        &mut nav_items,
        &root_config,
        &root_config.assets_dir,
    )?;

    // Strip number prefixes from output paths (used for URLs and output dirs).
    // Sorting has already happened with original paths, so this is safe.
    for album in &mut albums {
        album.path = slug_path(&album.path);
    }
    slugify_nav_paths(&mut nav_items);

    let description = read_description(root, &root_config.site_description_file)?;
    let pages = parse_pages(root, &root_config.site_description_file)?;

    // Root-level resolved config for CSS generation
    let config = root_config;

    Ok(Manifest {
        navigation: nav_items,
        albums,
        pages,
        description,
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
/// The `site_description_stem` file (e.g. `site.md`) is excluded — it is
/// rendered on the index page, not as a standalone page.
fn parse_pages(root: &Path, site_description_stem: &str) -> Result<Vec<Page>, ScanError> {
    let exclude_filename = format!("{}.md", site_description_stem);
    let mut md_files: Vec<PathBuf> = fs::read_dir(root)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .map(|e| e.eq_ignore_ascii_case("md"))
                    .unwrap_or(false)
                && p.file_name()
                    .map(|n| n.to_string_lossy() != exclude_filename)
                    .unwrap_or(true)
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
    assets_dir: &str,
) -> Result<(), ScanError> {
    let entries = collect_entries(path, if path == root { Some(assets_dir) } else { None })?;

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

        let source_dir_name = path.file_name().unwrap().to_string_lossy().to_string();
        albums.push(album);

        // Add to nav if numbered
        if in_nav {
            nav_items.push(NavItem {
                title,
                path: album_path,
                source_dir: source_dir_name,
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
            scan_directory(
                subdir,
                root,
                albums,
                &mut child_nav,
                &effective_config,
                assets_dir,
            )?;
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
                    source_dir: dir_name.to_string(),
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

fn collect_entries(path: &Path, assets_dir: Option<&str>) -> Result<Vec<PathBuf>, ScanError> {
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
                && assets_dir.is_none_or(|ad| *name != *ad)
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
    crate::imaging::supported_input_extensions().contains(&ext.as_str())
}

/// Read a description from `<stem>.md` or `<stem>.txt` in the given directory.
///
/// - `.md` takes priority and is rendered as markdown HTML.
/// - `.txt` is converted to HTML with smart paragraph handling and URL linkification.
/// - Returns `None` if neither file exists or contents are empty.
fn read_description(dir: &Path, stem: &str) -> Result<Option<String>, ScanError> {
    let md_path = dir.join(format!("{}.md", stem));
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

    let txt_path = dir.join(format!("{}.txt", stem));
    if txt_path.exists() {
        let content = fs::read_to_string(&txt_path)?.trim().to_string();
        if content.is_empty() {
            return Ok(None);
        }
        return Ok(Some(plain_text_to_html(&content)));
    }

    Ok(None)
}

/// Read an album description from `description.md` or `description.txt`.
fn read_album_description(album_dir: &Path) -> Result<Option<String>, ScanError> {
    read_description(album_dir, "description")
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

    // Detect thumb-designated images (name starts with "thumb", case-insensitive)
    let thumb_keys: Vec<u32> = numbered_images
        .iter()
        .filter(|(_, (_, parsed))| {
            let lower = parsed.name.to_ascii_lowercase();
            lower == "thumb" || lower.starts_with("thumb-")
        })
        .map(|(&key, _)| key)
        .collect();

    if thumb_keys.len() > 1 {
        return Err(ScanError::DuplicateThumb(path.to_path_buf()));
    }

    let thumb_key = thumb_keys.first().copied();

    // Find preview image: thumb > #1 > first by sort order
    let preview_image = if let Some(key) = thumb_key {
        numbered_images.get(&key).map(|(p, _)| *p).unwrap()
    } else {
        numbered_images
            .iter()
            .find(|&(&num, _)| num == 1)
            .map(|(_, (p, _))| *p)
            .or_else(|| numbered_images.values().next().map(|(p, _)| *p))
            // Safe: build_album is only called with non-empty images
            .unwrap()
    };

    let preview_rel = preview_image.strip_prefix(root).unwrap();

    // Build image list (exclude thumb-designated image — it's only used as preview)
    let images: Vec<Image> = numbered_images
        .iter()
        .filter(|&(&num, _)| thumb_key != Some(num))
        .map(|(&num, (img_path, parsed))| {
            let filename = img_path.file_name().unwrap().to_string_lossy().to_string();

            let title = if parsed.display_title.is_empty() {
                None
            } else {
                Some(parsed.display_title.clone())
            };
            let slug = parsed.name.clone();

            let source = img_path.strip_prefix(root).unwrap();
            let description = metadata::read_sidecar(img_path);
            Image {
                number: num,
                source_path: source.to_string_lossy().to_string(),
                filename,
                slug,
                title,
                description,
            }
        })
        .collect();

    // Read description: description.md takes priority over description.txt
    let description = read_album_description(path)?;

    // Detect supporting files
    let mut support_files = Vec::new();
    if path.join("config.toml").exists() {
        support_files.push("config.toml".to_string());
    }
    if path.join("description.md").exists() {
        support_files.push("description.md".to_string());
    } else if path.join("description.txt").exists() {
        support_files.push("description.txt".to_string());
    }

    Ok(Album {
        path: rel_path.to_string_lossy().to_string(),
        title,
        description,
        preview_image: preview_rel.to_string_lossy().to_string(),
        images,
        in_nav,
        config,
        support_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn scan_finds_all_albums() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        assert_eq!(
            album_titles(&manifest),
            vec!["Landscapes", "Japan", "Italy", "Minimal", "wip-drafts"]
        );
    }

    #[test]
    fn numbered_albums_appear_in_nav() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        assert_eq!(
            nav_titles(&manifest),
            vec!["Landscapes", "Travel", "Minimal"]
        );
    }

    #[test]
    fn unnumbered_albums_hidden_from_nav() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        assert!(!find_album(&manifest, "wip-drafts").in_nav);
    }

    #[test]
    fn fixture_full_nav_shape() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        assert_nav_shape(
            &manifest,
            &[
                ("Landscapes", &[]),
                ("Travel", &["Japan", "Italy"]),
                ("Minimal", &[]),
            ],
        );
    }

    #[test]
    fn images_sorted_by_number() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        let numbers: Vec<u32> = find_album(&manifest, "Landscapes")
            .images
            .iter()
            .map(|i| i.number)
            .collect();
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

        let desc = find_album(&manifest, "Landscapes")
            .description
            .as_ref()
            .unwrap();
        assert!(desc.contains("<p>"));
        assert!(desc.contains("landscape"));

        assert!(find_album(&manifest, "Minimal").description.is_none());
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
    fn preview_image_is_thumb() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        assert!(
            find_album(&manifest, "Landscapes")
                .preview_image
                .contains("005-thumb")
        );
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

        assert!(manifest.pages.len() >= 2);

        let about = find_page(&manifest, "about");
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

        let github = find_page(&manifest, "github");
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

        // Root config.toml overrides ALL defaults — verify a sample from each section
        assert_eq!(manifest.config.thumbnails.aspect_ratio, [3, 4]);
        assert_eq!(manifest.config.images.quality, 85);
        assert_eq!(manifest.config.images.sizes, vec![600, 1200, 1800]);
        assert_eq!(manifest.config.theme.thumbnail_gap, "0.75rem");
        assert_eq!(manifest.config.theme.mat_x.size, "4vw");
        assert_eq!(manifest.config.theme.mat_y.min, "1.5rem");
        assert_eq!(manifest.config.colors.light.background, "#fafafa");
        assert_eq!(manifest.config.colors.dark.text_muted, "#888888");
        assert_eq!(manifest.config.font.font, "Playfair Display");
        assert_eq!(manifest.config.font.weight, "400");
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
        assert_eq!(manifest.config.colors.dark.background, "#000000");
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

        let japan = find_album(&manifest, "Japan");
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
    fn fixture_per_gallery_config_overrides_root() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        let landscapes = find_album(&manifest, "Landscapes");
        // Landscapes has its own config.toml: quality=75, aspect_ratio=[1,1]
        assert_eq!(landscapes.config.images.quality, 75);
        assert_eq!(landscapes.config.thumbnails.aspect_ratio, [1, 1]);
        // Other values inherited from root config
        assert_eq!(landscapes.config.images.sizes, vec![600, 1200, 1800]);
        assert_eq!(landscapes.config.colors.light.background, "#fafafa");
    }

    #[test]
    fn fixture_album_without_config_inherits_root() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        let minimal = find_album(&manifest, "Minimal");
        assert_eq!(minimal.config.images.quality, 85);
        assert_eq!(minimal.config.thumbnails.aspect_ratio, [3, 4]);
    }

    #[test]
    fn fixture_config_chain_all_sections() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        // Landscapes overrides images.quality and thumbnails.aspect_ratio;
        // everything else should come from root config.
        let ls = find_album(&manifest, "Landscapes");

        // From Landscapes/config.toml
        assert_eq!(ls.config.images.quality, 75);
        assert_eq!(ls.config.thumbnails.aspect_ratio, [1, 1]);

        // Inherited from root config — theme
        assert_eq!(ls.config.theme.thumbnail_gap, "0.75rem");
        assert_eq!(ls.config.theme.grid_padding, "1.5rem");
        assert_eq!(ls.config.theme.mat_x.size, "4vw");
        assert_eq!(ls.config.theme.mat_x.min, "0.5rem");
        assert_eq!(ls.config.theme.mat_x.max, "3rem");
        assert_eq!(ls.config.theme.mat_y.size, "5vw");
        assert_eq!(ls.config.theme.mat_y.min, "1.5rem");
        assert_eq!(ls.config.theme.mat_y.max, "4rem");

        // Inherited from root config — colors
        assert_eq!(ls.config.colors.light.background, "#fafafa");
        assert_eq!(ls.config.colors.light.text_muted, "#777777");
        assert_eq!(ls.config.colors.light.border, "#d0d0d0");
        assert_eq!(ls.config.colors.light.link, "#444444");
        assert_eq!(ls.config.colors.light.link_hover, "#111111");
        assert_eq!(ls.config.colors.dark.background, "#111111");
        assert_eq!(ls.config.colors.dark.text, "#eeeeee");
        assert_eq!(ls.config.colors.dark.link, "#bbbbbb");

        // Inherited from root config — font
        assert_eq!(ls.config.font.font, "Playfair Display");
        assert_eq!(ls.config.font.weight, "400");
        assert_eq!(ls.config.font.font_type, crate::config::FontType::Serif);

        // Inherited from root config — image sizes (not overridden by gallery)
        assert_eq!(ls.config.images.sizes, vec![600, 1200, 1800]);
    }

    #[test]
    fn fixture_image_sidecar_read() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        // Landscapes: dawn has sidecar; dusk and night do not (thumb excluded from images)
        assert_eq!(
            image_descriptions(find_album(&manifest, "Landscapes")),
            vec![
                Some("First light breaking over the mountain ridge."),
                None,
                None,
            ]
        );

        // Japan: tokyo has a sidecar
        let tokyo = find_image(find_album(&manifest, "Japan"), "tokyo");
        assert_eq!(
            tokyo.description.as_deref(),
            Some("Shibuya crossing at dusk, long exposure.")
        );
    }

    #[test]
    fn fixture_image_titles() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        assert_eq!(
            image_titles(find_album(&manifest, "Landscapes")),
            vec![Some("dawn"), Some("dusk"), Some("night")]
        );
    }

    #[test]
    fn fixture_description_md_overrides_txt() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        let desc = find_album(&manifest, "Japan").description.as_ref().unwrap();
        assert!(desc.contains("<strong>Tokyo</strong>"));
        assert!(!desc.contains("Street photography"));
    }

    // =========================================================================
    // Wider input variants
    // =========================================================================

    #[test]
    fn http_link_page_detected() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("010-link.md"), "http://example.com\n").unwrap();

        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        let page = manifest.pages.first().unwrap();
        assert!(page.is_link);
    }

    #[test]
    fn preview_image_when_first_is_not_001() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("005-first.jpg"), "fake image").unwrap();
        fs::write(album.join("010-second.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        assert!(manifest.albums[0].preview_image.contains("005-first"));
    }

    #[test]
    fn description_md_preserves_inline_html() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();
        fs::write(
            album.join("description.md"),
            "Text with <em>emphasis</em> and a [link](https://example.com).",
        )
        .unwrap();

        let manifest = scan(tmp.path()).unwrap();
        let desc = manifest.albums[0].description.as_ref().unwrap();
        // Markdown renders inline HTML and markdown syntax
        assert!(desc.contains("<em>emphasis</em>"));
        assert!(desc.contains(r#"<a href="https://example.com">link</a>"#));
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

    // =========================================================================
    // Assets directory tests
    // =========================================================================

    #[test]
    fn assets_dir_skipped_during_scan() {
        let tmp = TempDir::new().unwrap();

        // Create a real album
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();

        // Create an assets directory with files that look like images
        let assets = tmp.path().join("assets");
        fs::create_dir_all(assets.join("fonts")).unwrap();
        fs::write(assets.join("favicon.ico"), "icon data").unwrap();
        fs::write(assets.join("001-should-not-scan.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();

        // Should only find the real album, not treat assets as an album
        assert_eq!(manifest.albums.len(), 1);
        assert_eq!(manifest.albums[0].title, "Test");
    }

    #[test]
    fn custom_assets_dir_skipped_during_scan() {
        let tmp = TempDir::new().unwrap();

        // Configure a custom assets dir
        fs::write(
            tmp.path().join("config.toml"),
            r#"assets_dir = "site-assets""#,
        )
        .unwrap();

        // Create a real album
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();

        // Create the custom assets directory
        let assets = tmp.path().join("site-assets");
        fs::create_dir_all(&assets).unwrap();
        fs::write(assets.join("001-nope.jpg"), "fake image").unwrap();

        // Also create a default "assets" dir — should NOT be skipped since config says "site-assets"
        let default_assets = tmp.path().join("assets");
        fs::create_dir_all(&default_assets).unwrap();
        fs::write(default_assets.join("001-also.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();

        // "assets" dir should be scanned as an album (not skipped) since config overrides
        // "site-assets" dir should be skipped
        let album_titles: Vec<&str> = manifest.albums.iter().map(|a| a.title.as_str()).collect();
        assert!(album_titles.contains(&"Test"));
        assert!(album_titles.contains(&"assets"));
        assert!(!album_titles.iter().any(|t| *t == "site-assets"));
    }

    // =========================================================================
    // Site description tests
    // =========================================================================

    #[test]
    fn site_description_read_from_md() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();
        fs::write(tmp.path().join("site.md"), "**Welcome** to the gallery.").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        let desc = manifest.description.as_ref().unwrap();
        assert!(desc.contains("<strong>Welcome</strong>"));
    }

    #[test]
    fn site_description_read_from_txt() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();
        fs::write(tmp.path().join("site.txt"), "A plain text description.").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        let desc = manifest.description.as_ref().unwrap();
        assert!(desc.contains("<p>A plain text description.</p>"));
    }

    #[test]
    fn site_description_md_takes_priority_over_txt() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();
        fs::write(tmp.path().join("site.md"), "Markdown version").unwrap();
        fs::write(tmp.path().join("site.txt"), "Text version").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        let desc = manifest.description.as_ref().unwrap();
        assert!(desc.contains("Markdown version"));
        assert!(!desc.contains("Text version"));
    }

    #[test]
    fn site_description_empty_returns_none() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();
        fs::write(tmp.path().join("site.md"), "  \n  ").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        assert!(manifest.description.is_none());
    }

    #[test]
    fn site_description_none_when_no_file() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        assert!(manifest.description.is_none());
    }

    #[test]
    fn site_description_excluded_from_pages() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();
        fs::write(tmp.path().join("site.md"), "# Site Description\n\nContent.").unwrap();
        fs::write(tmp.path().join("010-about.md"), "# About\n\nAbout page.").unwrap();

        let manifest = scan(tmp.path()).unwrap();

        // site.md should be in description, not in pages
        assert!(manifest.description.is_some());
        assert_eq!(manifest.pages.len(), 1);
        assert_eq!(manifest.pages[0].slug, "about");
    }

    #[test]
    fn site_description_custom_file_name() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("config.toml"),
            r#"site_description_file = "intro""#,
        )
        .unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-test.jpg"), "fake image").unwrap();
        fs::write(tmp.path().join("intro.md"), "Custom intro text.").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        let desc = manifest.description.as_ref().unwrap();
        assert!(desc.contains("Custom intro text."));
    }

    #[test]
    fn fixture_site_description_loaded() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        let desc = manifest.description.as_ref().unwrap();
        assert!(desc.contains("fine art photography"));
    }

    #[test]
    fn fixture_site_md_not_in_pages() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        // site.md should not appear as a page
        assert!(manifest.pages.iter().all(|p| p.slug != "site"));
    }

    // =========================================================================
    // Thumb image tests
    // =========================================================================

    #[test]
    fn thumb_image_overrides_preview() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-first.jpg"), "fake image").unwrap();
        fs::write(album.join("005-thumb.jpg"), "fake image").unwrap();
        fs::write(album.join("010-last.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        assert!(manifest.albums[0].preview_image.contains("005-thumb"));
    }

    #[test]
    fn thumb_image_excluded_from_images() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-first.jpg"), "fake image").unwrap();
        fs::write(album.join("003-thumb.jpg"), "fake image").unwrap();
        fs::write(album.join("005-last.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        // Thumb is used as preview
        assert!(manifest.albums[0].preview_image.contains("003-thumb"));
        // But NOT included in the image list
        assert_eq!(manifest.albums[0].images.len(), 2);
        assert!(manifest.albums[0].images.iter().all(|i| i.number != 3));
    }

    #[test]
    fn thumb_image_with_title_excluded_from_images() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-first.jpg"), "fake image").unwrap();
        fs::write(album.join("003-thumb-The-Sunset.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        // Preview uses the thumb image
        assert!(
            manifest.albums[0]
                .preview_image
                .contains("003-thumb-The-Sunset")
        );
        // Thumb is NOT in the image list
        assert_eq!(manifest.albums[0].images.len(), 1);
        assert_eq!(manifest.albums[0].images[0].number, 1);
    }

    #[test]
    fn duplicate_thumb_is_error() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("001-thumb.jpg"), "fake image").unwrap();
        fs::write(album.join("002-thumb-Other.jpg"), "fake image").unwrap();

        let result = scan(tmp.path());
        assert!(matches!(result, Err(ScanError::DuplicateThumb(_))));
    }

    #[test]
    fn no_thumb_falls_back_to_first() {
        let tmp = TempDir::new().unwrap();
        let album = tmp.path().join("010-Test");
        fs::create_dir_all(&album).unwrap();
        fs::write(album.join("005-first.jpg"), "fake image").unwrap();
        fs::write(album.join("010-second.jpg"), "fake image").unwrap();

        let manifest = scan(tmp.path()).unwrap();
        // No thumb, no image #1 → falls back to first by sort order (005)
        assert!(manifest.albums[0].preview_image.contains("005-first"));
    }
}
