//! Site configuration module.
//!
//! Handles loading and parsing the `config.toml` file from the content root directory.
//! Configuration is optional - sensible defaults are used when no config file exists.
//!
//! ## Config File Location
//!
//! Place `config.toml` in the content root directory:
//!
//! ```text
//! content/
//! ├── config.toml          # Site configuration
//! ├── about.md             # Optional about page
//! ├── 010-Landscapes/
//! │   └── ...
//! └── 020-Portraits/
//!     └── ...
//! ```
//!
//! ## Configuration Options
//!
//! ```toml
//! # All options are optional - defaults shown below
//!
//! content_root = "content"  # Path to content directory
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
//! max_processes = 4             # Max parallel workers (omit for auto = CPU cores)
//! ```
//!
//! ## Partial Configuration
//!
//! You can override just the values you want to change:
//!
//! ```toml
//! # Only override the light mode background
//! [colors.light]
//! background = "#fafafa"
//! ```

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
}

/// Site configuration loaded from config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteConfig {
    /// Path to the content root directory (where albums, config, and pages live)
    #[serde(default = "default_content_root")]
    pub content_root: String,
    pub colors: ColorConfig,
    pub thumbnails: ThumbnailsConfig,
    pub images: ImagesConfig,
    pub theme: ThemeConfig,
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

/// Parallel processing settings
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
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

/// Thumbnail generation settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThumbnailsConfig {
    /// Aspect ratio as [width, height], e.g. [4, 5] for portrait
    pub aspect_ratio: [u32; 2],
}

impl Default for ThumbnailsConfig {
    fn default() -> Self {
        Self {
            aspect_ratio: [4, 5],
        }
    }
}

/// Responsive image generation settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ImagesConfig {
    /// Responsive sizes to generate
    pub sizes: Vec<u32>,
    /// AVIF/WebP quality (0-100)
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
pub struct ClampSize {
    pub size: String,
    pub min: String,
    pub max: String,
}

impl ClampSize {
    /// Render as a CSS `clamp()` expression.
    pub fn to_css(&self) -> String {
        format!("clamp({}, {}, {})", self.min, self.size, self.max)
    }
}

/// Theme/layout settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    /// Horizontal frame padding around images (left/right)
    pub frame_x: ClampSize,
    /// Vertical frame padding around images (top/bottom)
    pub frame_y: ClampSize,
    /// Gap between thumbnails in both album and image grids
    pub thumbnail_gap: String,
    /// Padding around the thumbnail grid container
    pub grid_padding: String,
}

fn default_thumbnail_gap() -> String {
    "1rem".to_string()
}

fn default_grid_padding() -> String {
    "2rem".to_string()
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
            thumbnail_gap: default_thumbnail_gap(),
            grid_padding: default_grid_padding(),
        }
    }
}

/// Color configuration for light and dark modes
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorConfig {
    pub light: ColorScheme,
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

/// Individual color scheme
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorScheme {
    /// Background color
    pub background: String,
    /// Primary text color
    pub text: String,
    /// Muted/secondary text color (used for nav menu, breadcrumbs)
    pub text_muted: String,
    /// Border color
    pub border: String,
    /// Link color
    pub link: String,
    /// Link hover color
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

/// Load config from config.toml in the given directory
pub fn load_config(root: &Path) -> Result<SiteConfig, ConfigError> {
    let config_path = root.join("config.toml");
    if !config_path.exists() {
        return Ok(SiteConfig::default());
    }

    let content = fs::read_to_string(&config_path)?;
    let config: SiteConfig = toml::from_str(&content)?;
    Ok(config)
}

/// Generate CSS custom properties from color config
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

/// Generate CSS custom properties from theme config
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
}
