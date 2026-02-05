use crate::config::{self, SiteConfig};
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
    #[error("No preview image (001-*) found in album: {0}")]
    NoPreviewImage(PathBuf),
}

/// Manifest output from the scan stage
#[derive(Debug, Serialize)]
pub struct Manifest {
    pub navigation: Vec<NavItem>,
    pub albums: Vec<Album>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub about: Option<AboutPage>,
    pub config: SiteConfig,
}

/// About page content
#[derive(Debug, Serialize)]
pub struct AboutPage {
    /// Title derived from markdown content (first # heading)
    pub title: String,
    /// Link title derived from filename (dashes to spaces)
    pub link_title: String,
    /// Raw markdown content (will be converted to HTML in generate stage)
    pub body: String,
}

/// Navigation tree item (only numbered directories)
#[derive(Debug, Serialize)]
pub struct NavItem {
    pub title: String,
    pub path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<NavItem>,
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
#[derive(Debug, Serialize)]
pub struct Image {
    pub number: u32,
    pub source_path: String,
    pub filename: String,
}

const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];

pub fn scan(root: &Path) -> Result<Manifest, ScanError> {
    let mut albums = Vec::new();
    let mut nav_items = Vec::new();

    scan_directory(root, root, &mut albums, &mut nav_items)?;

    // Check for about page
    let about = parse_about_page(root)?;

    // Load site config (uses defaults if config.toml doesn't exist)
    let config = config::load_config(root)?;

    Ok(Manifest {
        navigation: nav_items,
        albums,
        about,
        config,
    })
}

/// Parse markdown file for about page if one exists in root directory
/// Link title comes from filename (dashes to spaces)
/// Page title comes from first # heading in markdown
/// Body is raw markdown content
fn parse_about_page(root: &Path) -> Result<Option<AboutPage>, ScanError> {
    // Find .md files in root directory
    let md_files: Vec<PathBuf> = fs::read_dir(root)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .map(|e| e.to_ascii_lowercase() == "md")
                    .unwrap_or(false)
        })
        .collect();

    let md_path = match md_files.first() {
        Some(p) => p,
        None => return Ok(None),
    };

    // Extract link title from filename (dashes to spaces, no extension)
    let filename = md_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "about".to_string());
    let link_title = filename.replace('-', " ");

    let content = fs::read_to_string(md_path)?;

    // Extract title from first # heading, or use link_title as fallback
    let title = content
        .lines()
        .find(|line| line.starts_with("# "))
        .map(|line| line.trim_start_matches("# ").trim().to_string())
        .unwrap_or_else(|| link_title.clone());

    // Body is the full markdown content (will be converted to HTML in generate stage)
    let body = content;

    Ok(Some(AboutPage {
        title,
        link_title,
        body,
    }))
}

fn scan_directory(
    path: &Path,
    root: &Path,
    albums: &mut Vec<Album>,
    nav_items: &mut Vec<NavItem>,
) -> Result<(), ScanError> {
    let entries = collect_entries(path)?;

    let images = entries
        .iter()
        .filter(|e| is_image(e))
        .collect::<Vec<_>>();

    let subdirs = entries
        .iter()
        .filter(|e| e.is_dir())
        .collect::<Vec<_>>();

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
            (
                parse_number_prefix(&d.file_name().unwrap().to_string_lossy()).unwrap_or(u32::MAX),
                d.file_name().unwrap().to_string_lossy().to_string(),
            )
        });

        for subdir in sorted_subdirs {
            scan_directory(subdir, root, albums, &mut child_nav)?;
        }

        // If this directory is numbered, add it to nav with children
        if path != root {
            let dir_name = path.file_name().unwrap().to_string_lossy();
            if let Some((_, title)) = parse_numbered_name(&dir_name) {
                let rel_path = path.strip_prefix(root).unwrap();
                nav_items.push(NavItem {
                    title,
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
        parse_number_prefix(&format!(
            "{:03}-{}",
            item.path
                .split('/')
                .next_back()
                .and_then(parse_number_prefix)
                .unwrap_or(0),
            &item.title
        ))
        .unwrap_or(u32::MAX)
    });

    Ok(())
}

fn collect_entries(path: &Path) -> Result<Vec<PathBuf>, ScanError> {
    let mut entries: Vec<PathBuf> = fs::read_dir(path)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().unwrap().to_string_lossy();
            // Skip hidden files, info.txt, config.toml, and build artifacts
            !name.starts_with('.')
                && name != "info.txt"
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

    let (in_nav, title) = if let Some((_, t)) = parse_numbered_name(&dir_name) {
        (true, t)
    } else {
        (false, dir_name.to_string())
    };

    // Parse image numbers and check for duplicates
    let mut numbered_images: BTreeMap<u32, &PathBuf> = BTreeMap::new();
    let mut unnumbered_counter = 0u32;
    for img in images {
        let filename = img.file_name().unwrap().to_string_lossy();
        if let Some(num) = parse_number_prefix(&filename) {
            if numbered_images.contains_key(&num) {
                return Err(ScanError::DuplicateNumber(num, path.to_path_buf()));
            }
            numbered_images.insert(num, img);
        } else {
            // Images without numbers get sorted to the end, preserving filename order
            let high_num = 1_000_000 + unnumbered_counter;
            unnumbered_counter += 1;
            numbered_images.insert(high_num, img);
        }
    }

    // Find preview image (001-*)
    let preview_image = numbered_images
        .iter()
        .find(|&(&num, _)| num == 1)
        .map(|(_, path)| *path)
        .or_else(|| numbered_images.values().next().copied())
        .ok_or_else(|| ScanError::NoPreviewImage(path.to_path_buf()))?;

    let preview_rel = preview_image.strip_prefix(root).unwrap();

    // Build image list
    let images: Vec<Image> = numbered_images
        .iter()
        .map(|(&num, img_path)| {
            let filename = img_path.file_name().unwrap().to_string_lossy().to_string();
            let source = img_path.strip_prefix(root).unwrap();
            Image {
                number: num,
                source_path: source.to_string_lossy().to_string(),
                filename,
            }
        })
        .collect();

    // Read description if present
    let info_path = path.join("info.txt");
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

/// Parse "NNN-title" format, returns (number, title)
fn parse_numbered_name(name: &str) -> Option<(u32, String)> {
    let parts: Vec<&str> = name.splitn(2, '-').collect();
    if parts.len() == 2
        && let Ok(num) = parts[0].parse::<u32>()
    {
        return Some((num, parts[1].to_string()));
    }
    None
}

/// Parse just the number prefix from a name
fn parse_number_prefix(name: &str) -> Option<u32> {
    let prefix: String = name.chars().take_while(|c| c.is_ascii_digit()).collect();
    prefix.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_fixtures() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/images");

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

        // Should find 4 albums: Landscapes, Japan, Italy, Minimal, wip-drafts
        assert_eq!(manifest.albums.len(), 5);
    }

    #[test]
    fn numbered_albums_appear_in_nav() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        // Top level nav should have: Landscapes, Travel, Minimal (all numbered)
        assert_eq!(manifest.navigation.len(), 3);

        let titles: Vec<&str> = manifest.navigation.iter().map(|n| n.title.as_str()).collect();
        assert!(titles.contains(&"Landscapes"));
        assert!(titles.contains(&"Travel"));
        assert!(titles.contains(&"Minimal"));
    }

    #[test]
    fn unnumbered_albums_hidden_from_nav() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        let wip = manifest.albums.iter().find(|a| a.title == "wip-drafts").unwrap();
        assert!(!wip.in_nav);
    }

    #[test]
    fn nested_albums_have_children_in_nav() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        let travel = manifest.navigation.iter().find(|n| n.title == "Travel").unwrap();
        assert_eq!(travel.children.len(), 2);

        let child_titles: Vec<&str> = travel.children.iter().map(|n| n.title.as_str()).collect();
        assert!(child_titles.contains(&"Japan"));
        assert!(child_titles.contains(&"Italy"));
    }

    #[test]
    fn images_sorted_by_number() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        let landscapes = manifest.albums.iter().find(|a| a.title == "Landscapes").unwrap();
        let numbers: Vec<u32> = landscapes.images.iter().map(|i| i.number).collect();

        assert_eq!(numbers, vec![1, 2, 10]);
    }

    #[test]
    fn description_read_from_info_txt() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        let landscapes = manifest.albums.iter().find(|a| a.title == "Landscapes").unwrap();
        assert!(landscapes.description.is_some());
        assert!(landscapes.description.as_ref().unwrap().contains("landscape"));

        let minimal = manifest.albums.iter().find(|a| a.title == "Minimal").unwrap();
        assert!(minimal.description.is_none());
    }

    #[test]
    fn preview_image_is_001() {
        let tmp = setup_fixtures();
        let manifest = scan(tmp.path()).unwrap();

        let landscapes = manifest.albums.iter().find(|a| a.title == "Landscapes").unwrap();
        assert!(landscapes.preview_image.contains("001-dawn"));
    }

    #[test]
    fn mixed_content_is_error() {
        let tmp = TempDir::new().unwrap();

        // Create a directory with both images and subdirs
        let mixed = tmp.path().join("010-Mixed");
        fs::create_dir_all(&mixed).unwrap();
        fs::create_dir_all(mixed.join("subdir")).unwrap();

        // Create a placeholder image in mixed dir
        let img_path = mixed.join("001-photo.jpg");
        std::process::Command::new("magick")
            .args(["-size", "1x1", "xc:gray", img_path.to_str().unwrap()])
            .output()
            .unwrap();

        let result = scan(tmp.path());
        assert!(matches!(result, Err(ScanError::MixedContent(_))));
    }

    #[test]
    fn duplicate_number_is_error() {
        let tmp = TempDir::new().unwrap();

        let album = tmp.path().join("010-Album");
        fs::create_dir_all(&album).unwrap();

        // Create two images with the same number
        for name in ["001-first.jpg", "001-second.jpg"] {
            let img_path = album.join(name);
            std::process::Command::new("magick")
                .args(["-size", "1x1", "xc:gray", img_path.to_str().unwrap()])
                .output()
                .unwrap();
        }

        let result = scan(tmp.path());
        assert!(matches!(result, Err(ScanError::DuplicateNumber(1, _))));
    }
}
