//! CLI output formatting for all pipeline stages.
//!
//! Each pipeline stage (scan, process, generate) produces a manifest. This module
//! formats those manifests as human-readable, tree-based terminal output showing
//! the gallery hierarchy with positional indices (e.g. `001`, `002`).
//!
//! ## Why Tree-Based Display
//!
//! The output mirrors the directory structure so photographers can verify that
//! their content was discovered correctly. Positional indices show navigation
//! order, container vs. leaf albums are visually distinct, and image counts
//! give a quick sanity check. Each stage's formatter shows progressively more
//! detail: scan shows discovery, process shows generated sizes, generate shows
//! output file paths.
//!
//! ## Output Format
//!
//! ```text
//! Albums
//! 001 Landscapes (5 photos) [010-Landscapes]
//!     [001-dawn.jpg 001-dawn.txt]
//! 002 Travel [020-Travel]
//!     001 Japan (3 photos) [010-Japan]: A trip through Tokyo...
//!     002 Italy (2 photos) [020-Italy]
//!
//! Pages
//!     001 About [about.md]
//!
//! Config
//!     config.toml
//!     assets/
//! ```
//!
//! ## Functions
//!
//! Each stage has a `format_*` function (returns `Vec<String>`) for testability
//! and a `print_*` wrapper that writes to stdout.

use crate::types::NavItem;
use std::path::Path;

// ============================================================================
// Helpers
// ============================================================================

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

/// Format a 1-based positional index as 3-digit zero-padded.
fn format_index(pos: usize) -> String {
    format!("{:0>3}", pos)
}

/// Return indentation string: 4 spaces per depth level.
fn indent(depth: usize) -> String {
    "    ".repeat(depth)
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
pub fn format_scan_output(manifest: &crate::scan::Manifest, source_root: &Path) -> Vec<String> {
    let mut lines = Vec::new();

    // Albums section
    lines.push("Albums".to_string());

    let tree_nodes = walk_nav_tree(&manifest.navigation);

    // Track which album paths we've shown via nav tree
    let mut shown_paths = std::collections::HashSet::new();

    for node in &tree_nodes {
        let prefix = format!("{}{}", indent(node.depth), format_index(node.position));

        if node.is_container {
            // Container directory (no images, just children)
            lines.push(format!(
                "{} {} [{}]",
                prefix,
                node.path.split('/').next_back().unwrap_or(&node.path),
                node.source_dir
            ));
        } else {
            // Leaf album — find matching album in manifest
            if let Some(album) = manifest.albums.iter().find(|a| a.path == node.path) {
                shown_paths.insert(&album.path);
                let photo_count = album.images.len();
                let mut line = format!(
                    "{} {} ({} photos) [{}]",
                    prefix, album.title, photo_count, node.source_dir
                );
                if let Some(ref desc) = album.description {
                    let plain = strip_html_tags(desc);
                    let truncated = truncate_desc(plain.trim(), 40);
                    line.push_str(&format!(": {}", truncated));
                }
                lines.push(line);

                // Support files
                for sf in &album.support_files {
                    lines.push(format!("{}    {}", indent(node.depth), sf));
                }

                // Images with sidecar info
                for img in &album.images {
                    let sidecar_path = source_root.join(&img.source_path).with_extension("txt");
                    let sidecar_info = if sidecar_path.exists() {
                        let sidecar_name = sidecar_path.file_name().unwrap().to_string_lossy();
                        format!(" [{} {}]", img.filename, sidecar_name)
                    } else {
                        format!(" [{}]", img.filename)
                    };
                    lines.push(format!("{}   {}", indent(node.depth), sidecar_info));
                }
            }
        }
    }

    // Un-navigated albums
    for album in &manifest.albums {
        if !shown_paths.contains(&album.path) {
            let photo_count = album.images.len();
            let dir_name = album.path.split('/').next_back().unwrap_or(&album.path);
            let mut line = format!("    {} ({} photos)", dir_name, photo_count);
            if let Some(ref desc) = album.description {
                let plain = strip_html_tags(desc);
                let truncated = truncate_desc(plain.trim(), 40);
                line.push_str(&format!(": {}", truncated));
            }
            lines.push(line);
        }
    }

    // Pages section
    if !manifest.pages.is_empty() {
        lines.push(String::new());
        lines.push("Pages".to_string());
        for (i, page) in manifest.pages.iter().enumerate() {
            let idx = format_index(i + 1);
            let link_marker = if page.is_link { " (link)" } else { "" };
            let filename = format!("[{}.md]", page.slug);
            lines.push(format!(
                "    {} {}{} {}",
                idx, page.title, link_marker, filename
            ));
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

/// Format process stage output showing generated image sizes.
pub fn format_process_output(manifest: &crate::process::OutputManifest) -> Vec<String> {
    let mut lines = Vec::new();

    let tree_nodes = walk_nav_tree(&manifest.navigation);
    let mut shown_paths = std::collections::HashSet::new();
    let mut total_images = 0;

    for node in &tree_nodes {
        let prefix = format!("{}{}", indent(node.depth), format_index(node.position));

        if node.is_container {
            lines.push(format!(
                "{} {}",
                prefix,
                node.path.split('/').next_back().unwrap_or(&node.path)
            ));
        } else if let Some(album) = manifest.albums.iter().find(|a| a.path == node.path) {
            shown_paths.insert(&album.path);
            lines.push(format!(
                "{} {} ({} photos)",
                prefix,
                album.title,
                album.images.len()
            ));
            total_images += album.images.len();

            for img in &album.images {
                let sizes: Vec<String> = img.generated.keys().cloned().collect();
                let sizes_str = sizes.join(" ");
                let title_part = img.title.as_deref().unwrap_or(&img.slug);
                lines.push(format!(
                    "{}    {} → {} + thumb",
                    indent(node.depth),
                    title_part,
                    sizes_str
                ));
            }
        }
    }

    // Un-navigated albums
    for album in &manifest.albums {
        if !shown_paths.contains(&album.path) {
            lines.push(format!(
                "    {} ({} photos)",
                album.title,
                album.images.len()
            ));
            total_images += album.images.len();

            for img in &album.images {
                let sizes: Vec<String> = img.generated.keys().cloned().collect();
                let sizes_str = sizes.join(" ");
                let title_part = img.title.as_deref().unwrap_or(&img.slug);
                lines.push(format!("        {} → {} + thumb", title_part, sizes_str));
            }
        }
    }

    lines.push(format!(
        "Processed {} albums, {} images",
        manifest.albums.len(),
        total_images
    ));

    lines
}

/// Print process output to stdout.
pub fn print_process_output(manifest: &crate::process::OutputManifest) {
    for line in format_process_output(manifest) {
        println!("{}", line);
    }
}

// ============================================================================
// Stage 3: Generate output
// ============================================================================

/// Format generate stage output showing generated HTML files.
pub fn format_generate_output(manifest: &crate::generate::Manifest) -> Vec<String> {
    let mut lines = Vec::new();
    let mut total_image_pages = 0;

    // Home page
    lines.push("Home → index.html".to_string());

    let tree_nodes = walk_nav_tree(&manifest.navigation);
    let mut shown_paths = std::collections::HashSet::new();

    for node in &tree_nodes {
        let prefix = format!("{}{}", indent(node.depth), format_index(node.position));

        if node.is_container {
            lines.push(format!(
                "{} {}",
                prefix,
                node.path.split('/').next_back().unwrap_or(&node.path)
            ));
        } else if let Some(album) = manifest.albums.iter().find(|a| a.path == node.path) {
            shown_paths.insert(&album.path);
            lines.push(format!(
                "{} {} → {}/index.html",
                prefix, album.title, album.path
            ));

            for (idx, image) in album.images.iter().enumerate() {
                let page_url = crate::generate::image_page_url(
                    idx + 1,
                    album.images.len(),
                    image.title.as_deref(),
                );
                lines.push(format!(
                    "{}    → {}/{}index.html",
                    indent(node.depth),
                    album.path,
                    page_url
                ));
                total_image_pages += 1;
            }
        }
    }

    // Un-navigated albums
    for album in &manifest.albums {
        if !shown_paths.contains(&album.path) {
            lines.push(format!("    {} → {}/index.html", album.title, album.path));

            for (idx, image) in album.images.iter().enumerate() {
                let page_url = crate::generate::image_page_url(
                    idx + 1,
                    album.images.len(),
                    image.title.as_deref(),
                );
                lines.push(format!("        → {}/{}index.html", album.path, page_url));
                total_image_pages += 1;
            }
        }
    }

    // Pages
    for page in &manifest.pages {
        if page.is_link {
            lines.push(format!("{} → (external)", page.title));
        } else {
            lines.push(format!("{} → {}.html", page.title, page.slug));
        }
    }

    let page_count = manifest.pages.iter().filter(|p| !p.is_link).count();
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
}
