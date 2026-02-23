//! Centralized filename parsing for the NNN-name convention.
//!
//! All entry types (albums, groups, images, pages) follow the same naming pattern:
//! an optional numeric prefix (`NNN-`) followed by a name. This module provides
//! a single parsing function that extracts both parts consistently.
//!
//! ## Display Titles
//!
//! Dashes in the name portion are converted to spaces for display (preserving
//! original case). The `name` (slug) field is lowercased with underscores
//! converted to hyphens for URL-friendly paths:
//! - `020-My-Best-Photos/` → slug "my-best-photos", title "My Best Photos"
//! - `001-My-Museum.jpg` → slug "my-museum", title "My Museum"
//! - `040-who-am-i.md` → slug "who-am-i", title "who am i"

/// Result of parsing a numbered entry name like `020-My-Best-Photos`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedName {
    /// Number prefix if present (e.g., `20` from `020-My-Best-Photos`)
    pub number: Option<u32>,
    /// URL-friendly slug: lowercased, underscores replaced with hyphens.
    /// For unnumbered entries, derived from the full input.
    pub name: String,
    /// Display title: name with dashes converted to spaces (preserves original case).
    pub display_title: String,
}

/// Normalize a name part into a URL-friendly slug: lowercase, underscores → hyphens.
fn slugify(s: &str) -> String {
    s.to_lowercase().replace('_', "-")
}

/// Parse an entry name following the `NNN-name` convention.
///
/// Handles these patterns:
/// - `"020-My-Best-Photos"` → number=Some(20), name="my-best-photos", display_title="My Best Photos"
/// - `"010-Landscapes"` → number=Some(10), name="landscapes", display_title="Landscapes"
/// - `"001"` → number=Some(1), name="", display_title=""
/// - `"001-"` → number=Some(1), name="", display_title=""
/// - `"Museum"` → number=None, name="museum", display_title="Museum"
/// - `"wip-drafts"` → number=None, name="wip-drafts", display_title="wip drafts"
pub fn parse_entry_name(name: &str) -> ParsedName {
    // Try splitting on first dash
    if let Some(dash_pos) = name.find('-') {
        let prefix = &name[..dash_pos];
        if let Ok(num) = prefix.parse::<u32>() {
            let raw = &name[dash_pos + 1..];
            return ParsedName {
                number: Some(num),
                name: slugify(raw),
                display_title: raw.replace('-', " "),
            };
        }
    }
    // Check if the entire string is a pure number (no dash)
    if let Ok(num) = name.parse::<u32>() {
        return ParsedName {
            number: Some(num),
            name: String::new(),
            display_title: String::new(),
        };
    }
    // No number prefix
    ParsedName {
        number: None,
        name: slugify(name),
        display_title: name.replace('-', " "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_with_multi_word_name() {
        let p = parse_entry_name("020-My-Best-Photos");
        assert_eq!(p.number, Some(20));
        assert_eq!(p.name, "my-best-photos");
        assert_eq!(p.display_title, "My Best Photos");
    }

    #[test]
    fn numbered_single_word() {
        let p = parse_entry_name("010-Landscapes");
        assert_eq!(p.number, Some(10));
        assert_eq!(p.name, "landscapes");
        assert_eq!(p.display_title, "Landscapes");
    }

    #[test]
    fn number_only_no_dash() {
        let p = parse_entry_name("001");
        assert_eq!(p.number, Some(1));
        assert_eq!(p.name, "");
        assert_eq!(p.display_title, "");
    }

    #[test]
    fn number_with_trailing_dash() {
        let p = parse_entry_name("001-");
        assert_eq!(p.number, Some(1));
        assert_eq!(p.name, "");
        assert_eq!(p.display_title, "");
    }

    #[test]
    fn unnumbered_single_word() {
        let p = parse_entry_name("Museum");
        assert_eq!(p.number, None);
        assert_eq!(p.name, "museum");
        assert_eq!(p.display_title, "Museum");
    }

    #[test]
    fn unnumbered_with_dashes() {
        let p = parse_entry_name("wip-drafts");
        assert_eq!(p.number, None);
        assert_eq!(p.name, "wip-drafts");
        assert_eq!(p.display_title, "wip drafts");
    }

    #[test]
    fn image_stem_numbered_with_title() {
        let p = parse_entry_name("001-Museum");
        assert_eq!(p.number, Some(1));
        assert_eq!(p.name, "museum");
        assert_eq!(p.display_title, "Museum");
    }

    #[test]
    fn image_stem_dashes_become_spaces() {
        let p = parse_entry_name("001-My-Museum");
        assert_eq!(p.number, Some(1));
        assert_eq!(p.name, "my-museum");
        assert_eq!(p.display_title, "My Museum");
    }

    #[test]
    fn page_name_dashes_become_spaces() {
        let p = parse_entry_name("040-who-am-i");
        assert_eq!(p.number, Some(40));
        assert_eq!(p.name, "who-am-i");
        assert_eq!(p.display_title, "who am i");
    }

    #[test]
    fn large_number_prefix() {
        let p = parse_entry_name("999-Last");
        assert_eq!(p.number, Some(999));
        assert_eq!(p.name, "last");
        assert_eq!(p.display_title, "Last");
    }

    #[test]
    fn zero_prefix() {
        let p = parse_entry_name("000-First");
        assert_eq!(p.number, Some(0));
        assert_eq!(p.name, "first");
        assert_eq!(p.display_title, "First");
    }

    #[test]
    fn underscores_become_hyphens_in_slug() {
        let p = parse_entry_name("010-My_Photos");
        assert_eq!(p.number, Some(10));
        assert_eq!(p.name, "my-photos");
        assert_eq!(p.display_title, "My_Photos");
    }
}
