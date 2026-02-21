//! CLI output formatting for all pipeline stages.
//!
//! # Information-First Display
//!
//! Output is **information-centric, not file-centric**. The primary display
//! for every entity (album, image, page) is its semantic identity — title and
//! positional index — with filesystem paths shown as secondary context via
//! indented `Source:` lines. This makes the output readable as a content
//! inventory while still letting users trace data back to specific files.
//!
//! # Entity Display Contract
//!
//! Every entity follows a consistent two-level pattern across all stages:
//!
//! 1. **Header line**: positional index + title (+ optional detail like photo count)
//! 2. **Context lines**: indented `Source:`, `Description:`, variant status, etc.
//!
//! Shared helpers ([`entity_header`], [`image_line`]) enforce this pattern so
//! scan, process, and generate output look consistent for the same entities.
//!
//! # Output Format
//!
//! ## Scan
//!
//! ```text
//! Albums
//! 001 Landscapes (5 photos)
//!     Source: 010-Landscapes/
//!     001 dawn
//!         Source: 001-dawn.jpg
//!         Description: 001-dawn.txt
//!     002 mountains
//!         Source: 010-mountains.jpg
//!
//! Pages
//! 001 About
//!     Source: about.md
//!
//! Config
//!     config.toml
//!     assets/
//! ```
//!
//! ## Process
//!
//! ```text
//! Landscapes (5 photos)
//!     001 dawn
//!         Source: 001-dawn.jpg
//!         800px: cached
//!         1400px: encoded
//!         thumbnail: cached
//! ```
//!
//! ## Generate
//!
//! ```text
//! Home → index.html
//! 001 Landscapes → Landscapes/index.html
//!     001 dawn → Landscapes/1-dawn/index.html
//!     002 mountains → Landscapes/2-mountains/index.html
//!
//! Pages
//! 001 About → about.html
//!
//! Generated 2 albums, 4 image pages, 1 page
//! ```
//!
//! # Architecture
//!
//! Each stage has a `format_*` function (returns `Vec<String>`) for testability
//! and a `print_*` wrapper that writes to stdout. Format functions are pure —
//! no I/O, no side effects.

use crate::types::NavItem;
use std::path::Path;

// ============================================================================
// Shared entity display helpers
// ============================================================================

/// Format a 1-based positional index as 3-digit zero-padded.
fn format_index(pos: usize) -> String {
    format!("{:0>3}", pos)
}

/// Return indentation string: 4 spaces per depth level.
fn indent(depth: usize) -> String {
    "    ".repeat(depth)
}

/// Format an entity header: positional index + title, with optional detail.
///
/// Used for albums (with photo count) and containers (without).
///
/// ```text
/// 001 Landscapes (5 photos)
/// 001 Travel
/// ```
fn entity_header(index: usize, title: &str, count: Option<usize>) -> String {
    match count {
        Some(n) => format!("{} {} ({} photos)", format_index(index), title, n),
        None => format!("{} {}", format_index(index), title),
    }
}

/// Format an image line: titled images show title, untitled show filename in parens.
///
/// ```text
/// 001 The Sunset        // titled
/// 001 (010.avif)        // untitled — filename IS the identity
/// ```
fn image_line(index: usize, title: Option<&str>, filename: &str) -> String {
    match title {
        Some(t) if !t.is_empty() => format!("{} {}", format_index(index), t),
        _ => format!("{} ({})", format_index(index), filename),
    }
}

/// Strip HTML tags from a string (simple angle-bracket stripping).
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }
    result
}

/// Truncate text to `max` characters, appending `...` if truncated.
fn truncate_desc(text: &str, max: usize) -> String {
    if text.len() <= max {
        text.to_string()
    } else {
        format!("{}...", &text[..max])
    }
}

// ============================================================================
// Tree walker
// ============================================================================

/// A flattened node from walking the NavItem tree.
struct TreeNode {
    depth: usize,
    position: usize,
    path: String,
    is_container: bool,
    source_dir: String,
}

/// Walk the navigation tree, assigning positional indices per sibling level.
/// Returns a flat list of nodes with depth and position for formatting.
fn walk_nav_tree(nav: &[NavItem]) -> Vec<TreeNode> {
    let mut nodes = Vec::new();
    walk_nav_tree_recursive(nav, 0, &mut nodes);
    nodes
}

fn walk_nav_tree_recursive(items: &[NavItem], depth: usize, nodes: &mut Vec<TreeNode>) {
    for (i, item) in items.iter().enumerate() {
        let is_container = !item.children.is_empty();
        nodes.push(TreeNode {
            depth,
            position: i + 1,
            path: item.path.clone(),
            is_container,
            source_dir: item.source_dir.clone(),
        });
        if is_container {
            walk_nav_tree_recursive(&item.children, depth + 1, nodes);
        }
    }
}

// ============================================================================
// Stage 1: Scan output
// ============================================================================

/// Format scan stage output showing discovered gallery structure.
///
/// Information-first: each entity leads with its positional index and title.
/// Source paths and description files are shown as indented context lines.
pub fn format_scan_output(manifest: &crate::scan::Manifest, source_root: &Path) -> Vec<String> {
    let mut lines = Vec::new();

    // Albums section
    lines.push("Albums".to_string());

    let tree_nodes = walk_nav_tree(&manifest.navigation);
    let mut shown_paths = std::collections::HashSet::new();

    for node in &tree_nodes {
        let base_indent = indent(node.depth);

        if node.is_container {
            let header = entity_header(
                node.position,
                node.path.split('/').next_back().unwrap_or(&node.path),
                None,
            );
            lines.push(format!("{}{}", base_indent, header));
            lines.push(format!("{}    Source: {}/", base_indent, node.source_dir));
        } else if let Some(album) = manifest.albums.iter().find(|a| a.path == node.path) {
            shown_paths.insert(&album.path);
            let photo_count = album.images.len();
            let header = entity_header(node.position, &album.title, Some(photo_count));
            lines.push(format!("{}{}", base_indent, header));
            lines.push(format!("{}    Source: {}/", base_indent, node.source_dir));

            // Album description (truncated preview)
            if let Some(ref desc) = album.description {
                let plain = strip_html_tags(desc);
                let truncated = truncate_desc(plain.trim(), 60);
                if !truncated.is_empty() {
                    lines.push(format!("{}    {}", base_indent, truncated));
                }
            }

            // Images
            for (i, img) in album.images.iter().enumerate() {
                let img_indent = format!("{}    ", base_indent);
                let img_header = image_line(i + 1, img.title.as_deref(), &img.filename);
                lines.push(format!("{}{}", img_indent, img_header));

                // Source (always shown for titled images; implicit for untitled)
                if img.title.is_some() {
                    lines.push(format!("{}    Source: {}", img_indent, img.filename));
                }

                // Description sidecar
                let sidecar_path = source_root.join(&img.source_path).with_extension("txt");
                if sidecar_path.exists() {
                    let sidecar_name = sidecar_path.file_name().unwrap().to_string_lossy();
                    lines.push(format!("{}    Description: {}", img_indent, sidecar_name));
                }
            }
        }
    }

    // Un-navigated albums (hidden from nav, no number prefix)
    for album in &manifest.albums {
        if !shown_paths.contains(&album.path) {
            let dir_name = album.path.split('/').next_back().unwrap_or(&album.path);
            let photo_count = album.images.len();
            lines.push(format!("    {} ({} photos)", dir_name, photo_count));
            if let Some(ref desc) = album.description {
                let plain = strip_html_tags(desc);
                let truncated = truncate_desc(plain.trim(), 60);
                if !truncated.is_empty() {
                    lines.push(format!("        {}", truncated));
                }
            }
        }
    }

    // Pages section
    if !manifest.pages.is_empty() {
        lines.push(String::new());
        lines.push("Pages".to_string());
        for (i, page) in manifest.pages.iter().enumerate() {
            let link_marker = if page.is_link { " (link)" } else { "" };
            lines.push(format!(
                "    {} {}{}",
                format_index(i + 1),
                page.title,
                link_marker
            ));
            lines.push(format!("        Source: {}.md", page.slug));
        }
    }

    // Config section
    lines.push(String::new());
    lines.push("Config".to_string());
    let config_path = source_root.join("config.toml");
    if config_path.exists() {
        lines.push("    config.toml".to_string());
    }
    let assets_path = source_root.join(&manifest.config.assets_dir);
    if assets_path.is_dir() {
        lines.push(format!("    {}/", manifest.config.assets_dir));
    }

    lines
}

/// Print scan output to stdout.
pub fn print_scan_output(manifest: &crate::scan::Manifest, source_root: &Path) {
    for line in format_scan_output(manifest, source_root) {
        println!("{}", line);
    }
}

// ============================================================================
// Stage 2: Process output
// ============================================================================

/// Format a single process progress event as display lines.
///
/// Information-first: each image leads with its positional index and title.
/// Source path and per-variant cache status are shown as indented context.
pub fn format_process_event(event: &crate::process::ProcessEvent) -> Vec<String> {
    use crate::process::{ProcessEvent, VariantStatus};
    match event {
        ProcessEvent::AlbumStarted { title, image_count } => {
            vec![format!("{} ({} photos)", title, image_count)]
        }
        ProcessEvent::ImageProcessed {
            index,
            title,
            source_path,
            variants,
        } => {
            let mut lines = Vec::new();
            let filename = Path::new(source_path)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| source_path.clone());

            lines.push(format!(
                "    {}",
                image_line(*index, title.as_deref(), &filename)
            ));
            lines.push(format!("        Source: {}", source_path));

            for variant in variants {
                let status_str = match &variant.status {
                    VariantStatus::Cached => "cached",
                    VariantStatus::Copied => "copied",
                    VariantStatus::Encoded => "encoded",
                };
                lines.push(format!("        {}: {}", variant.label, status_str));
            }
            lines
        }
    }
}

// ============================================================================
// Stage 3: Generate output
// ============================================================================

/// Format generate stage output showing generated HTML files.
///
/// Information-first: each entity leads with its positional index and title,
/// followed by `→` and the output path.
pub fn format_generate_output(manifest: &crate::generate::Manifest) -> Vec<String> {
    let mut lines = Vec::new();
    let mut total_image_pages = 0;

    // Home page
    lines.push("Home \u{2192} index.html".to_string());

    let tree_nodes = walk_nav_tree(&manifest.navigation);
    let mut shown_paths = std::collections::HashSet::new();

    for node in &tree_nodes {
        let base_indent = indent(node.depth);

        if node.is_container {
            let header = entity_header(
                node.position,
                node.path.split('/').next_back().unwrap_or(&node.path),
                None,
            );
            lines.push(format!("{}{}", base_indent, header));
        } else if let Some(album) = manifest.albums.iter().find(|a| a.path == node.path) {
            shown_paths.insert(&album.path);
            let header = entity_header(node.position, &album.title, None);
            lines.push(format!(
                "{}{} \u{2192} {}/index.html",
                base_indent, header, album.path
            ));

            for (idx, image) in album.images.iter().enumerate() {
                let page_url = crate::generate::image_page_url(
                    idx + 1,
                    album.images.len(),
                    image.title.as_deref(),
                );
                let display = match &image.title {
                    Some(t) if !t.is_empty() => format!("{} {}", format_index(idx + 1), t),
                    _ => format_index(idx + 1),
                };
                lines.push(format!(
                    "{}    {} \u{2192} {}/{}index.html",
                    base_indent, display, album.path, page_url
                ));
                total_image_pages += 1;
            }
        }
    }

    // Un-navigated albums
    for album in &manifest.albums {
        if !shown_paths.contains(&album.path) {
            lines.push(format!(
                "    {} \u{2192} {}/index.html",
                album.title, album.path
            ));

            for (idx, image) in album.images.iter().enumerate() {
                let page_url = crate::generate::image_page_url(
                    idx + 1,
                    album.images.len(),
                    image.title.as_deref(),
                );
                let display = match &image.title {
                    Some(t) if !t.is_empty() => format!("{} {}", format_index(idx + 1), t),
                    _ => format_index(idx + 1),
                };
                lines.push(format!(
                    "        {} \u{2192} {}/{}index.html",
                    display, album.path, page_url
                ));
                total_image_pages += 1;
            }
        }
    }

    // Pages section
    let page_count = manifest.pages.iter().filter(|p| !p.is_link).count();
    if !manifest.pages.is_empty() {
        lines.push(String::new());
        lines.push("Pages".to_string());
        for (i, page) in manifest.pages.iter().enumerate() {
            if page.is_link {
                lines.push(format!(
                    "    {} {} \u{2192} (external link)",
                    format_index(i + 1),
                    page.title
                ));
            } else {
                lines.push(format!(
                    "    {} {} \u{2192} {}.html",
                    format_index(i + 1),
                    page.title,
                    page.slug
                ));
            }
        }
    }

    lines.push(format!(
        "Generated {} albums, {} image pages, {} pages",
        manifest.albums.len(),
        total_image_pages,
        page_count
    ));

    lines
}

/// Print generate output to stdout.
pub fn print_generate_output(manifest: &crate::generate::Manifest) {
    for line in format_generate_output(manifest) {
        println!("{}", line);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Helper tests
    // =========================================================================

    #[test]
    fn strip_html_tags_removes_tags() {
        assert_eq!(strip_html_tags("<p>Hello <b>world</b></p>"), "Hello world");
    }

    #[test]
    fn strip_html_tags_no_tags() {
        assert_eq!(strip_html_tags("plain text"), "plain text");
    }

    #[test]
    fn strip_html_tags_empty() {
        assert_eq!(strip_html_tags(""), "");
    }

    #[test]
    fn strip_html_tags_nested() {
        assert_eq!(
            strip_html_tags("<div><p>Some <em>text</em></p></div>"),
            "Some text"
        );
    }

    #[test]
    fn truncate_desc_short() {
        assert_eq!(truncate_desc("Short text", 40), "Short text");
    }

    #[test]
    fn truncate_desc_exact() {
        let text = "a".repeat(40);
        assert_eq!(truncate_desc(&text, 40), text);
    }

    #[test]
    fn truncate_desc_long() {
        let text = "a".repeat(50);
        let expected = format!("{}...", "a".repeat(40));
        assert_eq!(truncate_desc(&text, 40), expected);
    }

    #[test]
    fn truncate_desc_empty() {
        assert_eq!(truncate_desc("", 40), "");
    }

    #[test]
    fn format_index_single_digit() {
        assert_eq!(format_index(1), "001");
    }

    #[test]
    fn format_index_double_digit() {
        assert_eq!(format_index(42), "042");
    }

    #[test]
    fn format_index_triple_digit() {
        assert_eq!(format_index(100), "100");
    }

    // =========================================================================
    // Entity display helper tests
    // =========================================================================

    #[test]
    fn entity_header_with_count() {
        assert_eq!(
            entity_header(1, "Landscapes", Some(5)),
            "001 Landscapes (5 photos)"
        );
    }

    #[test]
    fn entity_header_without_count() {
        assert_eq!(entity_header(2, "Travel", None), "002 Travel");
    }

    #[test]
    fn image_line_with_title() {
        assert_eq!(
            image_line(1, Some("The Sunset"), "010-The-Sunset.avif"),
            "001 The Sunset"
        );
    }

    #[test]
    fn image_line_without_title() {
        assert_eq!(image_line(1, None, "010.avif"), "001 (010.avif)");
    }

    #[test]
    fn image_line_with_empty_title() {
        assert_eq!(image_line(1, Some(""), "010.avif"), "001 (010.avif)");
    }

    // =========================================================================
    // Tree walker tests
    // =========================================================================

    #[test]
    fn walk_nav_tree_empty() {
        let nodes = walk_nav_tree(&[]);
        assert!(nodes.is_empty());
    }

    #[test]
    fn walk_nav_tree_flat() {
        let nav = vec![
            NavItem {
                title: "A".to_string(),
                path: "a".to_string(),
                source_dir: "010-A".to_string(),
                children: vec![],
            },
            NavItem {
                title: "B".to_string(),
                path: "b".to_string(),
                source_dir: "020-B".to_string(),
                children: vec![],
            },
        ];
        let nodes = walk_nav_tree(&nav);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].position, 1);
        assert_eq!(nodes[0].depth, 0);
        assert_eq!(nodes[0].path, "a");
        assert!(!nodes[0].is_container);
        assert_eq!(nodes[1].position, 2);
        assert_eq!(nodes[1].depth, 0);
    }

    #[test]
    fn walk_nav_tree_nested() {
        let nav = vec![NavItem {
            title: "Parent".to_string(),
            path: "parent".to_string(),
            source_dir: "010-Parent".to_string(),
            children: vec![
                NavItem {
                    title: "Child A".to_string(),
                    path: "parent/child-a".to_string(),
                    source_dir: "010-Child-A".to_string(),
                    children: vec![],
                },
                NavItem {
                    title: "Child B".to_string(),
                    path: "parent/child-b".to_string(),
                    source_dir: "020-Child-B".to_string(),
                    children: vec![],
                },
            ],
        }];
        let nodes = walk_nav_tree(&nav);
        assert_eq!(nodes.len(), 3);
        // Parent
        assert_eq!(nodes[0].position, 1);
        assert_eq!(nodes[0].depth, 0);
        assert!(nodes[0].is_container);
        // Child A
        assert_eq!(nodes[1].position, 1);
        assert_eq!(nodes[1].depth, 1);
        assert!(!nodes[1].is_container);
        // Child B
        assert_eq!(nodes[2].position, 2);
        assert_eq!(nodes[2].depth, 1);
    }

    #[test]
    fn indent_zero() {
        assert_eq!(indent(0), "");
    }

    #[test]
    fn indent_one() {
        assert_eq!(indent(1), "    ");
    }

    #[test]
    fn indent_two() {
        assert_eq!(indent(2), "        ");
    }

    // =========================================================================
    // Process event formatting tests
    // =========================================================================

    #[test]
    fn format_process_album_started() {
        use crate::process::ProcessEvent;
        let event = ProcessEvent::AlbumStarted {
            title: "Landscapes".to_string(),
            image_count: 5,
        };
        let lines = format_process_event(&event);
        assert_eq!(lines, vec!["Landscapes (5 photos)"]);
    }

    #[test]
    fn format_process_image_with_title() {
        use crate::process::{ProcessEvent, VariantInfo, VariantStatus};
        let event = ProcessEvent::ImageProcessed {
            index: 1,
            title: Some("The Sunset".to_string()),
            source_path: "010-Landscapes/001-sunset.jpg".to_string(),
            variants: vec![
                VariantInfo {
                    label: "800px".to_string(),
                    status: VariantStatus::Cached,
                },
                VariantInfo {
                    label: "1400px".to_string(),
                    status: VariantStatus::Encoded,
                },
                VariantInfo {
                    label: "thumbnail".to_string(),
                    status: VariantStatus::Copied,
                },
            ],
        };
        let lines = format_process_event(&event);
        assert_eq!(lines[0], "    001 The Sunset");
        assert_eq!(lines[1], "        Source: 010-Landscapes/001-sunset.jpg");
        assert_eq!(lines[2], "        800px: cached");
        assert_eq!(lines[3], "        1400px: encoded");
        assert_eq!(lines[4], "        thumbnail: copied");
    }

    #[test]
    fn format_process_image_without_title() {
        use crate::process::{ProcessEvent, VariantInfo, VariantStatus};
        let event = ProcessEvent::ImageProcessed {
            index: 3,
            title: None,
            source_path: "002-NY/38.avif".to_string(),
            variants: vec![VariantInfo {
                label: "800px".to_string(),
                status: VariantStatus::Cached,
            }],
        };
        let lines = format_process_event(&event);
        assert_eq!(lines[0], "    003 (38.avif)");
        assert_eq!(lines[1], "        Source: 002-NY/38.avif");
    }
}
