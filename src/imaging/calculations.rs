//! Pure calculation functions for image dimensions.
//!
//! All functions here are pure and testable without any I/O or images.

/// Calculate thumbnail dimensions from aspect ratio and short edge size.
///
/// # Arguments
/// * `aspect` - Target aspect ratio as (width, height)
/// * `short_edge` - Size of the shorter edge in pixels
///
/// # Returns
/// * `(width, height)` - Final thumbnail dimensions
///
/// # Examples
/// ```
/// # use simple_gal::imaging::calculate_thumbnail_dimensions;
/// // 4:5 portrait with short edge 400px → 400x500
/// assert_eq!(calculate_thumbnail_dimensions((4, 5), 400), (400, 500));
///
/// // 16:9 landscape with short edge 180px → 320x180
/// assert_eq!(calculate_thumbnail_dimensions((16, 9), 180), (320, 180));
/// ```
pub fn calculate_thumbnail_dimensions(aspect: (u32, u32), short_edge: u32) -> (u32, u32) {
    let (aspect_w, aspect_h) = aspect;

    if aspect_w <= aspect_h {
        // Portrait or square: width is the short edge
        let w = short_edge;
        let h = (w as f64 * aspect_h as f64 / aspect_w as f64).round() as u32;
        (w, h)
    } else {
        // Landscape: height is the short edge
        let h = short_edge;
        let w = (h as f64 * aspect_w as f64 / aspect_h as f64).round() as u32;
        (w, h)
    }
}

/// Represents a single responsive size to generate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponsiveSize {
    /// Target size (longer edge).
    pub target: u32,
    /// Calculated output width.
    pub width: u32,
    /// Calculated output height.
    pub height: u32,
}

/// Calculate which responsive sizes to generate and their dimensions.
///
/// Filters out sizes larger than the original and calculates output dimensions
/// preserving aspect ratio. If all requested sizes exceed the original,
/// returns the original size as the only entry.
///
/// # Arguments
/// * `original` - Original image dimensions (width, height)
/// * `sizes` - Requested breakpoint sizes (on the longer edge)
///
/// # Returns
/// * Vector of sizes to generate with their dimensions
pub fn calculate_responsive_sizes(original: (u32, u32), sizes: &[u32]) -> Vec<ResponsiveSize> {
    let (orig_w, orig_h) = original;
    let longer_edge = orig_w.max(orig_h);

    let mut result: Vec<ResponsiveSize> = sizes
        .iter()
        .map(|&target_size| {
            // Cap at source size — never upscale
            let capped = target_size.min(longer_edge);

            let (out_w, out_h) = if orig_w >= orig_h {
                // Landscape or square
                let ratio = capped as f64 / orig_w as f64;
                (capped, (orig_h as f64 * ratio).round() as u32)
            } else {
                // Portrait
                let ratio = capped as f64 / orig_h as f64;
                ((orig_w as f64 * ratio).round() as u32, capped)
            };

            ResponsiveSize {
                target: capped,
                width: out_w,
                height: out_h,
            }
        })
        .collect();

    // Deduplicate (multiple sizes may cap to the same source dimensions).
    // Use a HashSet so non-adjacent duplicates are also removed.
    let mut seen = std::collections::HashSet::new();
    result.retain(|s| seen.insert(s.target));

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // calculate_thumbnail_dimensions tests
    // =========================================================================

    #[test]
    fn thumbnail_portrait_aspect() {
        // 4:5 with short edge 400 → 400x500
        assert_eq!(calculate_thumbnail_dimensions((4, 5), 400), (400, 500));
    }

    #[test]
    fn thumbnail_landscape_aspect() {
        // 16:9 with short edge 180 → 320x180
        assert_eq!(calculate_thumbnail_dimensions((16, 9), 180), (320, 180));
    }

    #[test]
    fn thumbnail_square_aspect() {
        // 1:1 with short edge 200 → 200x200
        assert_eq!(calculate_thumbnail_dimensions((1, 1), 200), (200, 200));
    }

    #[test]
    fn thumbnail_extreme_portrait() {
        // 1:3 with short edge 100 → 100x300
        assert_eq!(calculate_thumbnail_dimensions((1, 3), 100), (100, 300));
    }

    #[test]
    fn thumbnail_extreme_landscape() {
        // 3:1 with short edge 100 → 300x100
        assert_eq!(calculate_thumbnail_dimensions((3, 1), 100), (300, 100));
    }

    // =========================================================================
    // calculate_responsive_sizes tests
    // =========================================================================

    #[test]
    fn responsive_caps_at_source_size() {
        // 1000x800 landscape: 800 fits, 1400 and 2080 cap to 1000 (deduped)
        let sizes = calculate_responsive_sizes((1000, 800), &[800, 1400, 2080]);
        assert_eq!(sizes.len(), 2);
        assert_eq!(sizes[0].target, 800);
        assert_eq!(sizes[1].target, 1000); // capped from 1400/2080
        assert_eq!(sizes[1].width, 1000);
        assert_eq!(sizes[1].height, 800);
    }

    #[test]
    fn responsive_calculates_dimensions_landscape() {
        // 2000x1500 landscape, target 1000 on longer edge
        let sizes = calculate_responsive_sizes((2000, 1500), &[1000]);
        assert_eq!(sizes.len(), 1);
        assert_eq!(sizes[0].width, 1000);
        assert_eq!(sizes[0].height, 750); // 1500 * (1000/2000) = 750
    }

    #[test]
    fn responsive_calculates_dimensions_portrait() {
        // 1500x2000 portrait, target 1000 on longer edge
        let sizes = calculate_responsive_sizes((1500, 2000), &[1000]);
        assert_eq!(sizes.len(), 1);
        assert_eq!(sizes[0].width, 750); // 1500 * (1000/2000) = 750
        assert_eq!(sizes[0].height, 1000);
    }

    #[test]
    fn responsive_caps_all_when_all_exceed() {
        // 500x400, all sizes exceed — all cap to 500, deduped to one
        let sizes = calculate_responsive_sizes((500, 400), &[800, 1400, 2080]);
        assert_eq!(sizes.len(), 1);
        assert_eq!(sizes[0].target, 500);
        assert_eq!(sizes[0].width, 500);
        assert_eq!(sizes[0].height, 400);
    }

    #[test]
    fn responsive_preserves_order() {
        let sizes = calculate_responsive_sizes((3000, 2000), &[800, 1400, 2080]);
        assert_eq!(sizes.len(), 3);
        assert_eq!(sizes[0].target, 800);
        assert_eq!(sizes[1].target, 1400);
        assert_eq!(sizes[2].target, 2080);
    }

    #[test]
    fn responsive_empty_sizes_returns_empty() {
        let sizes = calculate_responsive_sizes((1000, 800), &[]);
        assert_eq!(sizes.len(), 0);
    }
}
