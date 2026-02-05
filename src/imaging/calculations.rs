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
/// # use lighttable::imaging::calculate_thumbnail_dimensions;
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

/// Calculate dimensions needed to fill a target area (resize before crop).
///
/// Returns dimensions that completely cover the target area while maintaining
/// the source aspect ratio. One dimension will match exactly, the other may exceed.
///
/// # Arguments
/// * `source` - Original image dimensions (width, height)
/// * `target` - Target area dimensions (width, height)
///
/// # Returns
/// * `(width, height)` - Fill dimensions (at least one matches target)
pub fn calculate_fill_dimensions(source: (u32, u32), target: (u32, u32)) -> (u32, u32) {
    let (src_w, src_h) = source;
    let (tgt_w, tgt_h) = target;

    let src_aspect = src_w as f64 / src_h as f64;
    let tgt_aspect = tgt_w as f64 / tgt_h as f64;

    if src_aspect > tgt_aspect {
        // Source is wider: height will match, width will exceed
        let h = tgt_h;
        let w = (h as f64 * src_aspect).round() as u32;
        (w, h)
    } else {
        // Source is taller: width will match, height will exceed
        let w = tgt_w;
        let h = (w as f64 / src_aspect).round() as u32;
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
        .filter(|&&size| size <= longer_edge)
        .map(|&target_size| {
            let (out_w, out_h) = if orig_w >= orig_h {
                // Landscape or square
                let ratio = target_size as f64 / orig_w as f64;
                (target_size, (orig_h as f64 * ratio).round() as u32)
            } else {
                // Portrait
                let ratio = target_size as f64 / orig_h as f64;
                ((orig_w as f64 * ratio).round() as u32, target_size)
            };

            ResponsiveSize {
                target: target_size,
                width: out_w,
                height: out_h,
            }
        })
        .collect();

    // If original is smaller than all requested sizes, use original
    if result.is_empty() {
        result.push(ResponsiveSize {
            target: longer_edge,
            width: orig_w,
            height: orig_h,
        });
    }

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
    // calculate_fill_dimensions tests
    // =========================================================================

    #[test]
    fn fill_wider_source_to_portrait_target() {
        // 800x600 (4:3) → 400x500 target
        // Source is wider, so height matches: 500, width = 500 * (4/3) = 667
        assert_eq!(calculate_fill_dimensions((800, 600), (400, 500)), (667, 500));
    }

    #[test]
    fn fill_taller_source_to_landscape_target() {
        // 600x800 (3:4) → 500x400 target
        // Source is taller, so width matches: 500, height = 500 * (4/3) = 667
        assert_eq!(calculate_fill_dimensions((600, 800), (500, 400)), (500, 667));
    }

    #[test]
    fn fill_same_aspect_ratio() {
        // 800x600 (4:3) → 400x300 target (also 4:3)
        // Perfect match
        assert_eq!(calculate_fill_dimensions((800, 600), (400, 300)), (400, 300));
    }

    #[test]
    fn fill_square_source_to_portrait() {
        // 400x400 (1:1) → 200x300 target
        // Source is wider (1:1 > 2:3), height matches: 300, width = 300
        assert_eq!(calculate_fill_dimensions((400, 400), (200, 300)), (300, 300));
    }

    // =========================================================================
    // calculate_responsive_sizes tests
    // =========================================================================

    #[test]
    fn responsive_filters_larger_sizes() {
        let sizes = calculate_responsive_sizes((1000, 800), &[800, 1400, 2080]);
        assert_eq!(sizes.len(), 1);
        assert_eq!(sizes[0].target, 800);
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
    fn responsive_falls_back_to_original_when_all_exceed() {
        // 500x400, all sizes exceed
        let sizes = calculate_responsive_sizes((500, 400), &[800, 1400, 2080]);
        assert_eq!(sizes.len(), 1);
        assert_eq!(sizes[0].target, 500); // Longer edge
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
    fn responsive_empty_sizes_returns_original() {
        let sizes = calculate_responsive_sizes((1000, 800), &[]);
        assert_eq!(sizes.len(), 1);
        assert_eq!(sizes[0].target, 1000);
    }
}
