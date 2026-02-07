//! Site configuration module.
//!
//! Handles loading, validating, and merging `config.toml` files. Configuration
//! is hierarchical: stock defaults are overridden by user config files at any
//! level of the directory tree (root → group → gallery).
//!
//! ## Config File Location
//!
//! Place `config.toml` in the content root and/or any album group or album directory:
//!
//! ```text
//! content/
//! ├── config.toml              # Root config (overrides stock defaults)
//! ├── 010-Landscapes/
//! │   └── ...
//! └── 020-Travel/
//!     ├── config.toml          # Group config (overrides root)
//!     ├── 010-Japan/
//!     │   ├── config.toml      # Gallery config (overrides group)
//!     │   └── ...
//!     └── 020-Italy/
//!         └── ...
//! ```
//!
//! ## Configuration Options
//!
//! ```toml
//! # All options are optional - defaults shown below
//!
//! content_root = "content"  # Path to content directory (root-level only)
//!
//! [thumbnails]
//! aspect_ratio = [4, 5]     # width:height ratio
//!
//! [images]
//! sizes = [800, 1400, 2080] # Responsive sizes to generate
//! quality = 90              # AVIF/WebP quality (0-100)
//!
//! [theme]
//! thumbnail_gap = "1rem"    # Gap between thumbnails in grids
//! grid_padding = "2rem"     # Padding around thumbnail grids
//!
//! [theme.frame_x]
//! size = "3vw"              # Preferred horizontal frame size
//! min = "1rem"              # Minimum horizontal frame size
//! max = "2.5rem"            # Maximum horizontal frame size
//!
//! [theme.frame_y]
//! size = "6vw"              # Preferred vertical frame size
//! min = "2rem"              # Minimum vertical frame size
//! max = "5rem"              # Maximum vertical frame size
//!
//! [colors.light]
//! background = "#ffffff"
//! text = "#111111"
//! text_muted = "#666666"    # Nav menu, breadcrumbs, captions
//! border = "#e0e0e0"
//! link = "#333333"
//! link_hover = "#000000"
//!
//! [colors.dark]
//! background = "#0a0a0a"
//! text = "#eeeeee"
//! text_muted = "#999999"
//! border = "#333333"
//! link = "#cccccc"
//! link_hover = "#ffffff"
//!
//! [processing]
//! max_processes = 4         # Max parallel workers (omit for auto = CPU cores)
//! ```
//!
//! ## Partial Configuration
//!
//! Config files are sparse — override just the values you want:
//!
//! ```toml
//! # Only override the light mode background
//! [colors.light]
//! background = "#fafafa"
//! ```
//!
//! Unknown keys are rejected to catch typos early.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("Config validation error: {0}")]
    Validation(String),
}

/// Site configuration loaded from `config.toml`.
///
/// All fields have sensible defaults. User config files need only specify
/// the values they want to override. Unknown keys are rejected.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SiteConfig {
    /// Path to the content root directory (only meaningful at root level).
    #[serde(default = "default_content_root")]
    pub content_root: String,
    /// Color schemes for light and dark modes.
    pub colors: ColorConfig,
    /// Thumbnail generation settings (aspect ratio).
    pub thumbnails: ThumbnailsConfig,
    /// Responsive image generation settings (sizes, quality).
    pub images: ImagesConfig,
    /// Theme/layout settings (frame padding, grid spacing).
    pub theme: ThemeConfig,
    /// Parallel processing settings.
    pub processing: ProcessingConfig,
}

fn default_content_root() -> String {
    "content".to_string()
}

impl Default for SiteConfig {
    fn default() -> Self {
        Self {
            content_root: default_content_root(),
            colors: ColorConfig::default(),
            thumbnails: ThumbnailsConfig::default(),
            images: ImagesConfig::default(),
            theme: ThemeConfig::default(),
            processing: ProcessingConfig::default(),
        }
    }
}

impl SiteConfig {
    /// Validate config values are within acceptable ranges.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.images.quality > 100 {
            return Err(ConfigError::Validation(
                "images.quality must be 0-100".into(),
            ));
        }
        if self.thumbnails.aspect_ratio[0] == 0 || self.thumbnails.aspect_ratio[1] == 0 {
            return Err(ConfigError::Validation(
                "thumbnails.aspect_ratio values must be non-zero".into(),
            ));
        }
        if self.images.sizes.is_empty() {
            return Err(ConfigError::Validation(
                "images.sizes must not be empty".into(),
            ));
        }
        Ok(())
    }
}

/// Parallel processing settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProcessingConfig {
    /// Maximum number of parallel image processing workers.
    /// When absent or null, defaults to the number of CPU cores.
    /// Values larger than the core count are clamped down.
    pub max_processes: Option<usize>,
}

/// Resolve the effective thread count from config.
///
/// - `None` → use all available cores
/// - `Some(n)` → use `min(n, cores)` (user can constrain down, not up)
pub fn effective_threads(config: &ProcessingConfig) -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    config.max_processes.map(|n| n.min(cores)).unwrap_or(cores)
}

/// Thumbnail generation settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThumbnailsConfig {
    /// Aspect ratio as `[width, height]`, e.g. `[4, 5]` for portrait thumbnails.
    pub aspect_ratio: [u32; 2],
}

impl Default for ThumbnailsConfig {
    fn default() -> Self {
        Self {
            aspect_ratio: [4, 5],
        }
    }
}

/// Responsive image generation settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ImagesConfig {
    /// Pixel widths (longer edge) to generate for responsive `<picture>` elements.
    pub sizes: Vec<u32>,
    /// AVIF/WebP encoding quality (0 = worst, 100 = best).
    pub quality: u32,
}

impl Default for ImagesConfig {
    fn default() -> Self {
        Self {
            sizes: vec![800, 1400, 2080],
            quality: 90,
        }
    }
}

/// A responsive CSS size expressed as `clamp(min, size, max)`.
///
/// - `size`: the preferred/fluid value, typically viewport-relative (e.g. `"3vw"`)
/// - `min`: the minimum bound (e.g. `"1rem"`)
/// - `max`: the maximum bound (e.g. `"2.5rem"`)
///
/// Generates `clamp(min, size, max)` in the output CSS.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClampSize {
    /// Preferred/fluid value, typically viewport-relative (e.g. `"3vw"`).
    pub size: String,
    /// Minimum bound (e.g. `"1rem"`).
    pub min: String,
    /// Maximum bound (e.g. `"2.5rem"`).
    pub max: String,
}

impl ClampSize {
    /// Render as a CSS `clamp()` expression.
    pub fn to_css(&self) -> String {
        format!("clamp({}, {}, {})", self.min, self.size, self.max)
    }
}

/// Theme/layout settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    /// Horizontal frame padding around images (left/right).
    pub frame_x: ClampSize,
    /// Vertical frame padding around images (top/bottom).
    pub frame_y: ClampSize,
    /// Gap between thumbnails in both album and image grids (CSS value).
    pub thumbnail_gap: String,
    /// Padding around the thumbnail grid container (CSS value).
    pub grid_padding: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            frame_x: ClampSize {
                size: "3vw".to_string(),
                min: "1rem".to_string(),
                max: "2.5rem".to_string(),
            },
            frame_y: ClampSize {
                size: "6vw".to_string(),
                min: "2rem".to_string(),
                max: "5rem".to_string(),
            },
            thumbnail_gap: "1rem".to_string(),
            grid_padding: "2rem".to_string(),
        }
    }
}

/// Color configuration for light and dark modes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColorConfig {
    /// Light mode color scheme.
    pub light: ColorScheme,
    /// Dark mode color scheme.
    pub dark: ColorScheme,
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self {
            light: ColorScheme::default_light(),
            dark: ColorScheme::default_dark(),
        }
    }
}

/// Individual color scheme (light or dark).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColorScheme {
    /// Background color.
    pub background: String,
    /// Primary text color.
    pub text: String,
    /// Muted/secondary text color (used for nav menu, breadcrumbs, captions).
    pub text_muted: String,
    /// Border color.
    pub border: String,
    /// Link color.
    pub link: String,
    /// Link hover color.
    pub link_hover: String,
}

impl ColorScheme {
    pub fn default_light() -> Self {
        Self {
            background: "#ffffff".to_string(),
            text: "#111111".to_string(),
            text_muted: "#666666".to_string(),
            border: "#e0e0e0".to_string(),
            link: "#333333".to_string(),
            link_hover: "#000000".to_string(),
        }
    }

    pub fn default_dark() -> Self {
        Self {
            background: "#0a0a0a".to_string(),
            text: "#eeeeee".to_string(),
            text_muted: "#999999".to_string(),
            border: "#333333".to_string(),
            link: "#cccccc".to_string(),
            link_hover: "#ffffff".to_string(),
        }
    }
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self::default_light()
    }
}

// =============================================================================
// Config loading, merging, and validation
// =============================================================================

/// Returns the stock default config as a `toml::Value::Table`.
///
/// This is the canonical representation of all default values, used as the
/// base layer for merging user overrides on top.
pub fn stock_defaults_value() -> toml::Value {
    toml::Value::try_from(SiteConfig::default()).expect("default config must serialize")
}

/// Recursively merge `overlay` on top of `base`.
///
/// - Tables are merged key-by-key (overlay keys override base keys).
/// - Non-table values in overlay replace base values entirely.
/// - Keys in base that are not in overlay are preserved.
pub fn merge_toml(base: toml::Value, overlay: toml::Value) -> toml::Value {
    match (base, overlay) {
        (toml::Value::Table(mut base_table), toml::Value::Table(overlay_table)) => {
            for (key, overlay_val) in overlay_table {
                let merged = match base_table.remove(&key) {
                    Some(base_val) => merge_toml(base_val, overlay_val),
                    None => overlay_val,
                };
                base_table.insert(key, merged);
            }
            toml::Value::Table(base_table)
        }
        (_, overlay) => overlay,
    }
}

/// Load a `config.toml` from a directory as a raw TOML value.
///
/// Returns `Ok(None)` if no `config.toml` exists in the directory.
/// Returns `Err` if the file exists but contains invalid TOML.
pub fn load_raw_config(path: &Path) -> Result<Option<toml::Value>, ConfigError> {
    let config_path = path.join("config.toml");
    if !config_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&config_path)?;
    let value: toml::Value = toml::from_str(&content)?;
    Ok(Some(value))
}

/// Merge an optional overlay onto a base value, then deserialize and validate.
///
/// Used to resolve a fully-merged config at any point in the directory hierarchy.
pub fn resolve_config(
    base: toml::Value,
    overlay: Option<toml::Value>,
) -> Result<SiteConfig, ConfigError> {
    let merged = match overlay {
        Some(ov) => merge_toml(base, ov),
        None => base,
    };
    let config: SiteConfig = merged.try_into()?;
    config.validate()?;
    Ok(config)
}

/// Load config from `config.toml` in the given directory.
///
/// Merges user values on top of stock defaults, rejects unknown keys,
/// and validates the result.
pub fn load_config(root: &Path) -> Result<SiteConfig, ConfigError> {
    let base = stock_defaults_value();
    let overlay = load_raw_config(root)?;
    resolve_config(base, overlay)
}

/// Returns a fully-commented stock `config.toml` with all keys and explanations.
///
/// Used by the `gen-config` CLI command.
pub fn stock_config_toml() -> &'static str {
    r##"# Simple Gal Configuration
# ========================
# All settings are optional. Remove or comment out any you don't need.
# Values shown below are the defaults.
#
# Config files can be placed at any level of the directory tree:
#   content/config.toml          -> root (overrides stock defaults)
#   content/020-Travel/config.toml -> group (overrides root)
#   content/020-Travel/010-Japan/config.toml -> gallery (overrides group)
#
# Each level only needs the keys it wants to override.
# Unknown keys will cause an error.

# Path to content directory (only meaningful at root level)
content_root = "content"

# ---------------------------------------------------------------------------
# Thumbnail generation
# ---------------------------------------------------------------------------
[thumbnails]
# Aspect ratio as [width, height] for thumbnail crops.
# Common choices: [1, 1] for square, [4, 5] for portrait, [3, 2] for landscape.
aspect_ratio = [4, 5]

# ---------------------------------------------------------------------------
# Responsive image generation
# ---------------------------------------------------------------------------
[images]
# Pixel widths (longer edge) to generate for responsive <picture> elements.
sizes = [800, 1400, 2080]

# AVIF/WebP encoding quality (0 = worst, 100 = best).
quality = 90

# ---------------------------------------------------------------------------
# Theme / layout
# ---------------------------------------------------------------------------
[theme]
# Gap between thumbnails in album and image grids (CSS value).
thumbnail_gap = "1rem"

# Padding around the thumbnail grid container (CSS value).
grid_padding = "2rem"

# Horizontal frame padding around images, as CSS clamp(min, size, max).
[theme.frame_x]
size = "3vw"
min = "1rem"
max = "2.5rem"

# Vertical frame padding around images, as CSS clamp(min, size, max).
[theme.frame_y]
size = "6vw"
min = "2rem"
max = "5rem"

# ---------------------------------------------------------------------------
# Colors - Light mode (prefers-color-scheme: light)
# ---------------------------------------------------------------------------
[colors.light]
background = "#ffffff"
text = "#111111"
text_muted = "#666666"    # Nav, breadcrumbs, captions
border = "#e0e0e0"
link = "#333333"
link_hover = "#000000"

# ---------------------------------------------------------------------------
# Colors - Dark mode (prefers-color-scheme: dark)
# ---------------------------------------------------------------------------
[colors.dark]
background = "#0a0a0a"
text = "#eeeeee"
text_muted = "#999999"
border = "#333333"
link = "#cccccc"
link_hover = "#ffffff"

# ---------------------------------------------------------------------------
# Processing
# ---------------------------------------------------------------------------
[processing]
# Maximum parallel image-processing workers.
# Omit or comment out to auto-detect (= number of CPU cores).
# max_processes = 4
"##
}

/// Generate CSS custom properties from color config.
pub fn generate_color_css(colors: &ColorConfig) -> String {
    format!(
        r#":root {{
    --color-bg: {light_bg};
    --color-text: {light_text};
    --color-text-muted: {light_text_muted};
    --color-border: {light_border};
    --color-link: {light_link};
    --color-link-hover: {light_link_hover};
}}

@media (prefers-color-scheme: dark) {{
    :root {{
        --color-bg: {dark_bg};
        --color-text: {dark_text};
        --color-text-muted: {dark_text_muted};
        --color-border: {dark_border};
        --color-link: {dark_link};
        --color-link-hover: {dark_link_hover};
    }}
}}"#,
        light_bg = colors.light.background,
        light_text = colors.light.text,
        light_text_muted = colors.light.text_muted,
        light_border = colors.light.border,
        light_link = colors.light.link,
        light_link_hover = colors.light.link_hover,
        dark_bg = colors.dark.background,
        dark_text = colors.dark.text,
        dark_text_muted = colors.dark.text_muted,
        dark_border = colors.dark.border,
        dark_link = colors.dark.link,
        dark_link_hover = colors.dark.link_hover,
    )
}

/// Generate CSS custom properties from theme config.
pub fn generate_theme_css(theme: &ThemeConfig) -> String {
    format!(
        r#":root {{
    --frame-width-x: {frame_x};
    --frame-width-y: {frame_y};
    --thumbnail-gap: {thumbnail_gap};
    --grid-padding: {grid_padding};
}}"#,
        frame_x = theme.frame_x.to_css(),
        frame_y = theme.frame_y.to_css(),
        thumbnail_gap = theme.thumbnail_gap,
        grid_padding = theme.grid_padding,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_config_has_colors() {
        let config = SiteConfig::default();
        assert_eq!(config.colors.light.background, "#ffffff");
        assert_eq!(config.colors.dark.background, "#0a0a0a");
    }

    #[test]
    fn default_config_has_content_root() {
        let config = SiteConfig::default();
        assert_eq!(config.content_root, "content");
    }

    #[test]
    fn default_config_has_image_settings() {
        let config = SiteConfig::default();
        assert_eq!(config.thumbnails.aspect_ratio, [4, 5]);
        assert_eq!(config.images.sizes, vec![800, 1400, 2080]);
        assert_eq!(config.images.quality, 90);
        assert_eq!(config.theme.frame_x.to_css(), "clamp(1rem, 3vw, 2.5rem)");
        assert_eq!(config.theme.frame_y.to_css(), "clamp(2rem, 6vw, 5rem)");
    }

    #[test]
    fn parse_partial_config() {
        let toml = r##"
[colors.light]
background = "#fafafa"
"##;
        let config: SiteConfig = toml::from_str(toml).unwrap();
        // Overridden value
        assert_eq!(config.colors.light.background, "#fafafa");
        // Default values preserved
        assert_eq!(config.colors.light.text, "#111111");
        assert_eq!(config.colors.dark.background, "#0a0a0a");
        // Image settings should be defaults
        assert_eq!(config.images.sizes, vec![800, 1400, 2080]);
    }

    #[test]
    fn parse_image_settings() {
        let toml = r##"
[thumbnails]
aspect_ratio = [1, 1]

[images]
sizes = [400, 800]
quality = 85
"##;
        let config: SiteConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.thumbnails.aspect_ratio, [1, 1]);
        assert_eq!(config.images.sizes, vec![400, 800]);
        assert_eq!(config.images.quality, 85);
        // Unspecified defaults preserved
        assert_eq!(config.colors.light.background, "#ffffff");
    }

    #[test]
    fn generate_css_uses_config_colors() {
        let mut colors = ColorConfig::default();
        colors.light.background = "#f0f0f0".to_string();
        colors.dark.background = "#1a1a1a".to_string();

        let css = generate_color_css(&colors);
        assert!(css.contains("--color-bg: #f0f0f0"));
        assert!(css.contains("--color-bg: #1a1a1a"));
    }

    // =========================================================================
    // load_config tests
    // =========================================================================

    #[test]
    fn load_config_returns_default_when_no_file() {
        let tmp = TempDir::new().unwrap();
        let config = load_config(tmp.path()).unwrap();

        assert_eq!(config.colors.light.background, "#ffffff");
        assert_eq!(config.colors.dark.background, "#0a0a0a");
    }

    #[test]
    fn load_config_reads_file() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");

        fs::write(
            &config_path,
            r##"
[colors.light]
background = "#123456"
text = "#abcdef"
"##,
        )
        .unwrap();

        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.colors.light.background, "#123456");
        assert_eq!(config.colors.light.text, "#abcdef");
        // Unspecified values should be defaults
        assert_eq!(config.colors.dark.background, "#0a0a0a");
    }

    #[test]
    fn load_config_full_config() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");

        fs::write(
            &config_path,
            r##"
[colors.light]
background = "#fff"
text = "#000"
text_muted = "#666"
border = "#ccc"
link = "#00f"
link_hover = "#f00"

[colors.dark]
background = "#111"
text = "#eee"
text_muted = "#888"
border = "#444"
link = "#88f"
link_hover = "#f88"
"##,
        )
        .unwrap();

        let config = load_config(tmp.path()).unwrap();

        // Light mode
        assert_eq!(config.colors.light.background, "#fff");
        assert_eq!(config.colors.light.text, "#000");
        assert_eq!(config.colors.light.link, "#00f");

        // Dark mode
        assert_eq!(config.colors.dark.background, "#111");
        assert_eq!(config.colors.dark.text, "#eee");
        assert_eq!(config.colors.dark.link, "#88f");
    }

    #[test]
    fn load_config_invalid_toml_is_error() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");

        fs::write(&config_path, "this is not valid toml [[[").unwrap();

        let result = load_config(tmp.path());
        assert!(matches!(result, Err(ConfigError::Toml(_))));
    }

    // =========================================================================
    // CSS generation tests
    // =========================================================================

    #[test]
    fn generate_css_includes_all_variables() {
        let colors = ColorConfig::default();
        let css = generate_color_css(&colors);

        // Check all CSS variables are present
        assert!(css.contains("--color-bg:"));
        assert!(css.contains("--color-text:"));
        assert!(css.contains("--color-text-muted:"));
        assert!(css.contains("--color-border:"));
        assert!(css.contains("--color-link:"));
        assert!(css.contains("--color-link-hover:"));
    }

    #[test]
    fn generate_css_includes_dark_mode_media_query() {
        let colors = ColorConfig::default();
        let css = generate_color_css(&colors);

        assert!(css.contains("@media (prefers-color-scheme: dark)"));
    }

    #[test]
    fn color_scheme_default_is_light() {
        let scheme = ColorScheme::default();
        assert_eq!(scheme.background, "#ffffff");
    }

    #[test]
    fn clamp_size_to_css() {
        let size = ClampSize {
            size: "3vw".to_string(),
            min: "1rem".to_string(),
            max: "2.5rem".to_string(),
        };
        assert_eq!(size.to_css(), "clamp(1rem, 3vw, 2.5rem)");
    }

    #[test]
    fn generate_theme_css_includes_frame_variables() {
        let theme = ThemeConfig::default();
        let css = generate_theme_css(&theme);
        assert!(css.contains("--frame-width-x: clamp(1rem, 3vw, 2.5rem)"));
        assert!(css.contains("--frame-width-y: clamp(2rem, 6vw, 5rem)"));
        assert!(css.contains("--thumbnail-gap: 1rem"));
        assert!(css.contains("--grid-padding: 2rem"));
    }

    #[test]
    fn parse_thumbnail_gap_and_grid_padding() {
        let toml = r#"
[theme]
thumbnail_gap = "0.5rem"
grid_padding = "1rem"
"#;
        let config: SiteConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.theme.thumbnail_gap, "0.5rem");
        assert_eq!(config.theme.grid_padding, "1rem");
    }

    #[test]
    fn default_thumbnail_gap_and_grid_padding() {
        let config = SiteConfig::default();
        assert_eq!(config.theme.thumbnail_gap, "1rem");
        assert_eq!(config.theme.grid_padding, "2rem");
    }

    // =========================================================================
    // Processing config tests
    // =========================================================================

    #[test]
    fn default_processing_config() {
        let config = ProcessingConfig::default();
        assert_eq!(config.max_processes, None);
    }

    #[test]
    fn effective_threads_auto() {
        let config = ProcessingConfig {
            max_processes: None,
        };
        let threads = effective_threads(&config);
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        assert_eq!(threads, cores);
    }

    #[test]
    fn effective_threads_clamped_to_cores() {
        let config = ProcessingConfig {
            max_processes: Some(99999),
        };
        let threads = effective_threads(&config);
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        assert_eq!(threads, cores);
    }

    #[test]
    fn effective_threads_user_constrains_down() {
        let config = ProcessingConfig {
            max_processes: Some(1),
        };
        assert_eq!(effective_threads(&config), 1);
    }

    #[test]
    fn parse_processing_config() {
        let toml = r#"
[processing]
max_processes = 4
"#;
        let config: SiteConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.processing.max_processes, Some(4));
    }

    #[test]
    fn parse_config_without_processing_uses_default() {
        let toml = r##"
[colors.light]
background = "#fafafa"
"##;
        let config: SiteConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.processing.max_processes, None);
    }

    // =========================================================================
    // merge_toml tests
    // =========================================================================

    #[test]
    fn merge_toml_scalar_override() {
        let base: toml::Value = toml::from_str(r#"quality = 90"#).unwrap();
        let overlay: toml::Value = toml::from_str(r#"quality = 70"#).unwrap();
        let merged = merge_toml(base, overlay);
        assert_eq!(merged.get("quality").unwrap().as_integer(), Some(70));
    }

    #[test]
    fn merge_toml_table_merge() {
        let base: toml::Value = toml::from_str(
            r#"
[images]
sizes = [800, 1400]
quality = 90
"#,
        )
        .unwrap();
        let overlay: toml::Value = toml::from_str(
            r#"
[images]
quality = 70
"#,
        )
        .unwrap();
        let merged = merge_toml(base, overlay);
        let images = merged.get("images").unwrap();
        assert_eq!(images.get("quality").unwrap().as_integer(), Some(70));
        // sizes preserved from base
        assert!(images.get("sizes").unwrap().as_array().unwrap().len() == 2);
    }

    #[test]
    fn merge_toml_preserves_base_keys() {
        let base: toml::Value = toml::from_str(
            r#"
a = 1
b = 2
"#,
        )
        .unwrap();
        let overlay: toml::Value = toml::from_str(r#"a = 10"#).unwrap();
        let merged = merge_toml(base, overlay);
        assert_eq!(merged.get("a").unwrap().as_integer(), Some(10));
        assert_eq!(merged.get("b").unwrap().as_integer(), Some(2));
    }

    #[test]
    fn merge_toml_deep_nested() {
        let base: toml::Value = toml::from_str(
            r##"
[colors.light]
background = "#fff"
text = "#000"
"##,
        )
        .unwrap();
        let overlay: toml::Value = toml::from_str(
            r##"
[colors.light]
background = "#fafafa"
"##,
        )
        .unwrap();
        let merged = merge_toml(base, overlay);
        let light = merged.get("colors").unwrap().get("light").unwrap();
        assert_eq!(light.get("background").unwrap().as_str(), Some("#fafafa"));
        assert_eq!(light.get("text").unwrap().as_str(), Some("#000"));
    }

    #[test]
    fn merge_toml_three_layers() {
        let stock: toml::Value = toml::from_str(
            r#"
[images]
quality = 90
sizes = [800, 1400, 2080]
"#,
        )
        .unwrap();
        let root: toml::Value = toml::from_str(
            r#"
[images]
quality = 85
"#,
        )
        .unwrap();
        let gallery: toml::Value = toml::from_str(
            r#"
[images]
quality = 70
"#,
        )
        .unwrap();

        let merged = merge_toml(merge_toml(stock, root), gallery);
        let images = merged.get("images").unwrap();
        assert_eq!(images.get("quality").unwrap().as_integer(), Some(70));
        // sizes preserved from stock
        assert_eq!(images.get("sizes").unwrap().as_array().unwrap().len(), 3);
    }

    // =========================================================================
    // Unknown key rejection tests
    // =========================================================================

    #[test]
    fn unknown_key_rejected() {
        let toml_str = r#"
[images]
qualty = 90
"#;
        let result: Result<SiteConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown field"));
    }

    #[test]
    fn unknown_section_rejected() {
        let toml_str = r#"
[imagez]
quality = 90
"#;
        let result: Result<SiteConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn unknown_nested_key_rejected() {
        let toml_str = r##"
[colors.light]
bg = "#fff"
"##;
        let result: Result<SiteConfig, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    #[test]
    fn unknown_key_rejected_via_load_config() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("config.toml"),
            r#"
[images]
qualty = 90
"#,
        )
        .unwrap();

        let result = load_config(tmp.path());
        assert!(result.is_err());
    }

    // =========================================================================
    // Validation tests
    // =========================================================================

    #[test]
    fn validate_quality_boundary_ok() {
        let mut config = SiteConfig::default();
        config.images.quality = 100;
        assert!(config.validate().is_ok());

        config.images.quality = 0;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn validate_quality_too_high() {
        let mut config = SiteConfig::default();
        config.images.quality = 101;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("quality"));
    }

    #[test]
    fn validate_aspect_ratio_zero() {
        let mut config = SiteConfig::default();
        config.thumbnails.aspect_ratio = [0, 5];
        assert!(config.validate().is_err());

        config.thumbnails.aspect_ratio = [4, 0];
        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_sizes_empty() {
        let mut config = SiteConfig::default();
        config.images.sizes = vec![];
        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_default_config_passes() {
        let config = SiteConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn load_config_validates_values() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("config.toml"),
            r#"
[images]
quality = 200
"#,
        )
        .unwrap();

        let result = load_config(tmp.path());
        assert!(matches!(result, Err(ConfigError::Validation(_))));
    }

    // =========================================================================
    // resolve_config / load_raw_config tests
    // =========================================================================

    #[test]
    fn load_raw_config_returns_none_when_no_file() {
        let tmp = TempDir::new().unwrap();
        let result = load_raw_config(tmp.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn load_raw_config_returns_value_when_file_exists() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("config.toml"),
            r#"
[images]
quality = 85
"#,
        )
        .unwrap();

        let result = load_raw_config(tmp.path()).unwrap();
        assert!(result.is_some());
        let val = result.unwrap();
        assert_eq!(
            val.get("images")
                .unwrap()
                .get("quality")
                .unwrap()
                .as_integer(),
            Some(85)
        );
    }

    #[test]
    fn resolve_config_with_no_overlay() {
        let base = stock_defaults_value();
        let config = resolve_config(base, None).unwrap();
        assert_eq!(config.images.quality, 90);
        assert_eq!(config.colors.light.background, "#ffffff");
    }

    #[test]
    fn resolve_config_with_overlay() {
        let base = stock_defaults_value();
        let overlay: toml::Value = toml::from_str(
            r#"
[images]
quality = 70
"#,
        )
        .unwrap();
        let config = resolve_config(base, Some(overlay)).unwrap();
        assert_eq!(config.images.quality, 70);
        // Other fields preserved from defaults
        assert_eq!(config.images.sizes, vec![800, 1400, 2080]);
    }

    #[test]
    fn resolve_config_rejects_invalid_values() {
        let base = stock_defaults_value();
        let overlay: toml::Value = toml::from_str(
            r#"
[images]
quality = 200
"#,
        )
        .unwrap();
        let result = resolve_config(base, Some(overlay));
        assert!(matches!(result, Err(ConfigError::Validation(_))));
    }

    // =========================================================================
    // stock_config_toml tests
    // =========================================================================

    #[test]
    fn stock_config_toml_is_valid_toml() {
        let content = stock_config_toml();
        let _: toml::Value = toml::from_str(content).expect("stock config must be valid TOML");
    }

    #[test]
    fn stock_config_toml_roundtrips_to_defaults() {
        let content = stock_config_toml();
        let config: SiteConfig = toml::from_str(content).unwrap();
        assert_eq!(config.images.quality, 90);
        assert_eq!(config.images.sizes, vec![800, 1400, 2080]);
        assert_eq!(config.thumbnails.aspect_ratio, [4, 5]);
        assert_eq!(config.colors.light.background, "#ffffff");
        assert_eq!(config.colors.dark.background, "#0a0a0a");
        assert_eq!(config.theme.thumbnail_gap, "1rem");
    }

    #[test]
    fn stock_config_toml_contains_all_sections() {
        let content = stock_config_toml();
        assert!(content.contains("[thumbnails]"));
        assert!(content.contains("[images]"));
        assert!(content.contains("[theme]"));
        assert!(content.contains("[theme.frame_x]"));
        assert!(content.contains("[theme.frame_y]"));
        assert!(content.contains("[colors.light]"));
        assert!(content.contains("[colors.dark]"));
        assert!(content.contains("[processing]"));
    }

    // =========================================================================
    // stock_defaults_value tests
    // =========================================================================

    #[test]
    fn stock_defaults_value_is_table() {
        let val = stock_defaults_value();
        assert!(val.is_table());
    }

    #[test]
    fn stock_defaults_value_has_all_sections() {
        let val = stock_defaults_value();
        assert!(val.get("images").is_some());
        assert!(val.get("thumbnails").is_some());
        assert!(val.get("colors").is_some());
        assert!(val.get("theme").is_some());
        assert!(val.get("processing").is_some());
    }
}
