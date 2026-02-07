//! Centralized filename parsing for the NNN-name convention.
//!
//! All entry types (albums, groups, images, pages) follow the same naming pattern:
//! an optional numeric prefix (`NNN-`) followed by a name. This module provides
//! a single parsing function that extracts both parts consistently.
//!
//! ## Display Titles
//!
//! Dashes in the name portion are converted to spaces for display. This applies
//! uniformly to all entry types:
//! - `020-My-Best-Photos/` → "My Best Photos" (album title)
//! - `001-My-Museum.jpg` → "My Museum" (image title)
//! - `040-who-am-i.md` → "who am i" (page link title)

/// Result of parsing a numbered entry name like `020-My-Best-Photos`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedName {
    /// Number prefix if present (e.g., `20` from `020-My-Best-Photos`)
    pub number: Option<u32>,
    /// Raw name part after `NNN-`, dashes preserved. Empty if number-only.
    /// For unnumbered entries, this is the full input.
    pub name: String,
    /// Display title: name with dashes converted to spaces.
    pub display_title: String,
}

/// Parse an entry name following the `NNN-name` convention.
///
/// Handles these patterns:
/// - `"020-My-Best-Photos"` → number=Some(20), name="My-Best-Photos", display_title="My Best Photos"
/// - `"010-Landscapes"` → number=Some(10), name="Landscapes", display_title="Landscapes"
/// - `"001"` → number=Some(1), name="", display_title=""
/// - `"001-"` → number=Some(1), name="", display_title=""
/// - `"Museum"` → number=None, name="Museum", display_title="Museum"
/// - `"wip-drafts"` → number=None, name="wip-drafts", display_title="wip drafts"
pub fn parse_entry_name(name: &str) -> ParsedName {
    // Try splitting on first dash
    if let Some(dash_pos) = name.find('-') {
        let prefix = &name[..dash_pos];
        if let Ok(num) = prefix.parse::<u32>() {
            let raw = &name[dash_pos + 1..];
            return ParsedName {
                number: Some(num),
                name: raw.to_string(),
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
        name: name.to_string(),
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
        assert_eq!(p.name, "My-Best-Photos");
        assert_eq!(p.display_title, "My Best Photos");
    }

    #[test]
    fn numbered_single_word() {
        let p = parse_entry_name("010-Landscapes");
        assert_eq!(p.number, Some(10));
        assert_eq!(p.name, "Landscapes");
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
        assert_eq!(p.name, "Museum");
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
        assert_eq!(p.name, "Museum");
        assert_eq!(p.display_title, "Museum");
    }

    #[test]
    fn image_stem_dashes_become_spaces() {
        let p = parse_entry_name("001-My-Museum");
        assert_eq!(p.number, Some(1));
        assert_eq!(p.name, "My-Museum");
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
        assert_eq!(p.display_title, "Last");
    }

    #[test]
    fn zero_prefix() {
        let p = parse_entry_name("000-First");
        assert_eq!(p.number, Some(0));
        assert_eq!(p.display_title, "First");
    }
}
