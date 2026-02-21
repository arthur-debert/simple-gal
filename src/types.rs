//! Shared types used across all pipeline stages.
//!
//! These types are serialized to JSON between stages (scan → process → generate)
//! and must be identical across all three modules.

use serde::{Deserialize, Serialize};

/// A page generated from a markdown file in the content root.
///
/// Pages follow the same numbering convention as albums:
/// - Numbered files (`NNN-name.md`) appear in navigation, sorted by number
/// - Unnumbered files are generated but hidden from navigation
///
/// If the file content is just a URL, the page becomes an external link in nav.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    /// Title from first `# heading` in markdown, or link_title as fallback
    pub title: String,
    /// Display label in nav (filename with number stripped and dashes → spaces)
    pub link_title: String,
    /// URL slug (filename stem with number prefix stripped)
    pub slug: String,
    /// Raw markdown content (or URL for link pages)
    pub body: String,
    /// Whether this page appears in navigation (has number prefix)
    pub in_nav: bool,
    /// Sort key from number prefix (for ordering)
    pub sort_key: u32,
    /// If true, body is a URL and this page is an external link
    pub is_link: bool,
}

/// Navigation tree item (only numbered directories).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavItem {
    pub title: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<NavItem>,
}
