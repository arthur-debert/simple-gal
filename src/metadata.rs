//! Image metadata extraction and resolution.
//!
//! Each image can carry metadata (title, description) from two independent sources:
//!
//! ## Filesystem sources (read during scan phase)
//!
//! - **Title**: Derived from the filename stem via the `NNN-name` convention.
//!   `001-My-Photo.jpg` becomes "My Photo". Simple, requires no tooling, and
//!   consistent with album and page naming.
//!
//! - **Description**: Read from a sidecar text file with the same stem as the image.
//!   `001-My-Photo.txt` alongside `001-My-Photo.jpg`. Follows the same pattern
//!   as `info.txt` for album descriptions — plain text, no special format.
//!
//! ## Embedded metadata sources (read during process phase)
//!
//! - **Title**: IPTC Object Name (`IPTC:2:05`). This is the "Title" field in
//!   Lightroom, Capture One, and most DAM (Digital Asset Management) software.
//!
//! - **Description**: IPTC Caption-Abstract (`IPTC:2:120`). The standard "Caption"
//!   field in Lightroom. This is by far the most-used text metadata field among
//!   photographers — "Headline" and "Extended Description" exist in the IPTC spec
//!   but are journalism holdovers rarely used in fine art workflows.
//!
//! ## Resolution priority
//!
//! Each field is resolved independently. The first non-empty value wins:
//!
//! - **Title**: EXIF title → filename title → None
//! - **Description**: sidecar `.txt` → EXIF caption → None
//!
//! The rationale: embedded metadata represents deliberate curation in a photography
//! tool (the photographer typed it into Lightroom on purpose) and should win over
//! mechanical filename extraction. For descriptions, sidecar files are explicit
//! overrides — the user created a file on purpose — so they trump embedded metadata.
//!
//! ## Title sanitization
//!
//! Since resolved titles may end up in URLs and filenames (via the image page slug),
//! EXIF-sourced titles are sanitized for safe use: truncated to a reasonable length,
//! non-URL-safe characters replaced with dashes, consecutive dashes collapsed.
//! This prevents filesystem errors from long titles and broken URLs from special
//! characters.

use std::path::Path;

/// Resolve a metadata field from multiple sources.
///
/// Takes a list of optional values in priority order and returns the first
/// non-None, non-empty value. This is the core merge operation used for
/// both title and description resolution.
///
/// ```text
/// title:       resolve(&[exif_title,   filename_title])
/// description: resolve(&[sidecar_text, exif_caption])
/// ```
pub fn resolve(sources: &[Option<&str>]) -> Option<String> {
    sources
        .iter()
        .filter_map(|opt| {
            opt.map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
        })
        .next()
}

/// Read a sidecar `.txt` file for an image.
///
/// Given a path like `content/album/001-photo.jpg`, looks for
/// `content/album/001-photo.txt` and returns its trimmed contents.
/// Returns `None` if the file doesn't exist or is empty.
pub fn read_sidecar(image_path: &Path) -> Option<String> {
    let sidecar = image_path.with_extension("txt");
    std::fs::read_to_string(sidecar)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

const MAX_SLUG_LEN: usize = 80;

/// Sanitize a title string for use in URLs and filenames.
///
/// - Replaces non-alphanumeric characters (except dashes) with dashes
/// - Collapses consecutive dashes into one
/// - Strips leading and trailing dashes
/// - Truncates to `MAX_SLUG_LEN` characters (breaks at last dash before limit)
pub fn sanitize_slug(title: &str) -> String {
    let slug: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();

    // Collapse consecutive dashes
    let mut collapsed = String::with_capacity(slug.len());
    let mut prev_dash = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_dash {
                collapsed.push('-');
            }
            prev_dash = true;
        } else {
            collapsed.push(c);
            prev_dash = false;
        }
    }

    // Strip leading/trailing dashes
    let trimmed = collapsed.trim_matches('-');

    // Truncate at word boundary (last dash before limit)
    if trimmed.len() <= MAX_SLUG_LEN {
        trimmed.to_string()
    } else {
        let truncated = &trimmed[..MAX_SLUG_LEN];
        match truncated.rfind('-') {
            Some(pos) => truncated[..pos].to_string(),
            None => truncated.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // =========================================================================
    // resolve() tests
    // =========================================================================

    #[test]
    fn resolve_picks_first_non_none() {
        assert_eq!(
            resolve(&[Some("EXIF Title"), Some("Filename Title")]),
            Some("EXIF Title".to_string())
        );
    }

    #[test]
    fn resolve_skips_none() {
        assert_eq!(
            resolve(&[None, Some("Fallback")]),
            Some("Fallback".to_string())
        );
    }

    #[test]
    fn resolve_skips_empty_strings() {
        assert_eq!(
            resolve(&[Some(""), Some("Fallback")]),
            Some("Fallback".to_string())
        );
    }

    #[test]
    fn resolve_skips_whitespace_only() {
        assert_eq!(
            resolve(&[Some("  \n\t  "), Some("Fallback")]),
            Some("Fallback".to_string())
        );
    }

    #[test]
    fn resolve_returns_none_when_all_none() {
        assert_eq!(resolve(&[None, None]), None);
    }

    #[test]
    fn resolve_returns_none_for_empty_sources() {
        assert_eq!(resolve(&[]), None);
    }

    #[test]
    fn resolve_trims_whitespace() {
        assert_eq!(
            resolve(&[Some("  Padded Title  ")]),
            Some("Padded Title".to_string())
        );
    }

    // =========================================================================
    // read_sidecar() tests
    // =========================================================================

    #[test]
    fn read_sidecar_finds_matching_txt() {
        let dir = TempDir::new().unwrap();
        let img = dir.path().join("001-photo.jpg");
        let txt = dir.path().join("001-photo.txt");
        fs::write(&img, b"fake image").unwrap();
        fs::write(&txt, "A beautiful sunset over the mountains").unwrap();

        assert_eq!(
            read_sidecar(&img),
            Some("A beautiful sunset over the mountains".to_string())
        );
    }

    #[test]
    fn read_sidecar_returns_none_when_no_file() {
        let dir = TempDir::new().unwrap();
        let img = dir.path().join("001-photo.jpg");
        assert_eq!(read_sidecar(&img), None);
    }

    #[test]
    fn read_sidecar_returns_none_for_empty_file() {
        let dir = TempDir::new().unwrap();
        let img = dir.path().join("001-photo.jpg");
        let txt = dir.path().join("001-photo.txt");
        fs::write(&img, b"fake image").unwrap();
        fs::write(&txt, "").unwrap();
        assert_eq!(read_sidecar(&img), None);
    }

    #[test]
    fn read_sidecar_returns_none_for_whitespace_only() {
        let dir = TempDir::new().unwrap();
        let img = dir.path().join("001-photo.jpg");
        let txt = dir.path().join("001-photo.txt");
        fs::write(&img, b"fake image").unwrap();
        fs::write(&txt, "   \n  \t  ").unwrap();
        assert_eq!(read_sidecar(&img), None);
    }

    #[test]
    fn read_sidecar_trims_content() {
        let dir = TempDir::new().unwrap();
        let img = dir.path().join("001-photo.jpg");
        let txt = dir.path().join("001-photo.txt");
        fs::write(&img, b"fake image").unwrap();
        fs::write(&txt, "\n  Some description  \n").unwrap();

        assert_eq!(read_sidecar(&img), Some("Some description".to_string()));
    }

    // =========================================================================
    // sanitize_slug() tests
    // =========================================================================

    #[test]
    fn sanitize_slug_alphanumeric_passthrough() {
        assert_eq!(sanitize_slug("hello-world"), "hello-world");
        assert_eq!(sanitize_slug("Photo123"), "Photo123");
    }

    #[test]
    fn sanitize_slug_replaces_spaces_and_special_chars() {
        assert_eq!(sanitize_slug("My Great Photo!"), "My-Great-Photo");
        assert_eq!(sanitize_slug("Hello World"), "Hello-World");
        assert_eq!(sanitize_slug("foo@bar#baz"), "foo-bar-baz");
    }

    #[test]
    fn sanitize_slug_collapses_consecutive_dashes() {
        assert_eq!(sanitize_slug("a---b"), "a-b");
        assert_eq!(sanitize_slug("a - b"), "a-b");
        assert_eq!(sanitize_slug("hello   world"), "hello-world");
    }

    #[test]
    fn sanitize_slug_strips_leading_trailing_dashes() {
        assert_eq!(sanitize_slug("--hello--"), "hello");
        assert_eq!(sanitize_slug("  hello  "), "hello");
        assert_eq!(sanitize_slug("---"), "");
    }

    #[test]
    fn sanitize_slug_truncates_long_titles() {
        let long_title = "a-".repeat(50); // 100 chars
        let result = sanitize_slug(&long_title);
        assert!(result.len() <= MAX_SLUG_LEN);
        assert!(!result.ends_with('-'));
    }

    #[test]
    fn sanitize_slug_truncates_at_word_boundary() {
        // 85 chars, should truncate to last dash before 80
        let title = "this-is-a-very-long-title-that-exceeds-the-maximum-slug-length-and-should-be-truncated-here";
        let result = sanitize_slug(title);
        assert!(result.len() <= MAX_SLUG_LEN);
        assert!(!result.contains("truncated"));
    }

    #[test]
    fn sanitize_slug_handles_unicode() {
        assert_eq!(sanitize_slug("café"), "caf");
        assert_eq!(sanitize_slug("日本語"), "");
        assert_eq!(sanitize_slug("München"), "M-nchen");
    }

    #[test]
    fn sanitize_slug_empty_for_all_special_chars() {
        assert_eq!(sanitize_slug("@#$%"), "");
        assert_eq!(sanitize_slug("!!!"), "");
    }

    #[test]
    fn sanitize_slug_preserves_existing_dashes() {
        assert_eq!(sanitize_slug("my-photo-title"), "my-photo-title");
    }
}
