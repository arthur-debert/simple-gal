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
//! site_title = "Gallery"    # Breadcrumb home label and index page title
//!
//! [thumbnails]
//! aspect_ratio = [4, 5]     # width:height ratio
//!
//! [images]
//! sizes = [800, 1400, 2080] # Responsive sizes to generate
//! quality = 90              # AVIF quality (0-100)
//!
//! [theme]
//! thumbnail_gap = "1rem"    # Gap between thumbnails in grids
//! grid_padding = "2rem"     # Padding around thumbnail grids
//!
//! [theme.mat_x]
//! size = "3vw"              # Preferred horizontal mat size
//! min = "1rem"              # Minimum horizontal mat size
//! max = "2.5rem"            # Maximum horizontal mat size
//!
//! [theme.mat_y]
//! size = "6vw"              # Preferred vertical mat size
//! min = "2rem"              # Minimum vertical mat size
//! max = "5rem"              # Maximum vertical mat size
//!
//! [colors.light]
//! background = "#ffffff"
//! text = "#111111"
//! text_muted = "#666666"    # Nav menu, breadcrumbs, captions
//! border = "#e0e0e0"
//! separator = "#e0e0e0"
//! link = "#333333"
//! link_hover = "#000000"
//!
//! [colors.dark]
//! background = "#000000"
//! text = "#fafafa"
//! text_muted = "#999999"
//! border = "#333333"
//! separator = "#333333"
//! link = "#cccccc"
//! link_hover = "#ffffff"
//!
//! [font]
//! font = "Space Grotesk"    # Google Fonts family name
//! weight = "600"            # Font weight to load
//! font_type = "sans"        # "sans" or "serif" (determines fallbacks)
//!
//! [processing]
//! max_processes = 4         # Max parallel workers (omit for auto = CPU cores)
//!
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct SiteConfig {
    /// Path to the content root directory (only meaningful at root level).
    #[serde(default = "default_content_root")]
    pub content_root: String,
    /// Site title used in breadcrumbs and the browser tab for the home page.
    #[serde(default = "default_site_title")]
    pub site_title: String,
    /// Directory for static assets (favicon, fonts, etc.), relative to content root.
    /// Contents are copied verbatim to the output root during generation.
    #[serde(default = "default_assets_dir")]
    pub assets_dir: String,
    /// Stem of the site description file in the content root (e.g. "site" →
    /// looks for `site.md` / `site.txt`). Rendered on the index page.
    #[serde(default = "default_site_description_file")]
    pub site_description_file: String,
    /// Color schemes for light and dark modes.
    pub colors: ColorConfig,
    /// Thumbnail generation settings (aspect ratio).
    pub thumbnails: ThumbnailsConfig,
    /// Responsive image generation settings (sizes, quality).
    pub images: ImagesConfig,
    /// Theme/layout settings (frame padding, grid spacing).
    pub theme: ThemeConfig,
    /// Font configuration (Google Fonts or local font file).
    pub font: FontConfig,
    /// Parallel processing settings.
    pub processing: ProcessingConfig,
}

/// Partial site configuration for sparse loading and strict validation.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialSiteConfig {
    pub content_root: Option<String>,
    pub site_title: Option<String>,
    pub assets_dir: Option<String>,
    pub site_description_file: Option<String>,
    pub colors: Option<PartialColorConfig>,
    pub thumbnails: Option<PartialThumbnailsConfig>,
    pub images: Option<PartialImagesConfig>,
    pub theme: Option<PartialThemeConfig>,
    pub font: Option<PartialFontConfig>,
    pub processing: Option<PartialProcessingConfig>,
}

fn default_content_root() -> String {
    "content".to_string()
}

fn default_site_title() -> String {
    "Gallery".to_string()
}

fn default_assets_dir() -> String {
    "assets".to_string()
}

fn default_site_description_file() -> String {
    "site".to_string()
}

impl Default for SiteConfig {
    fn default() -> Self {
        Self {
            content_root: default_content_root(),
            site_title: default_site_title(),
            assets_dir: default_assets_dir(),
            site_description_file: default_site_description_file(),
            colors: ColorConfig::default(),
            thumbnails: ThumbnailsConfig::default(),
            images: ImagesConfig::default(),
            theme: ThemeConfig::default(),
            font: FontConfig::default(),
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

    /// Merge a partial config on top of this one.
    pub fn merge(mut self, other: PartialSiteConfig) -> Self {
        if let Some(cr) = other.content_root {
            self.content_root = cr;
        }
        if let Some(st) = other.site_title {
            self.site_title = st;
        }
        if let Some(ad) = other.assets_dir {
            self.assets_dir = ad;
        }
        if let Some(sd) = other.site_description_file {
            self.site_description_file = sd;
        }
        if let Some(c) = other.colors {
            self.colors = self.colors.merge(c);
        }
        if let Some(t) = other.thumbnails {
            self.thumbnails = self.thumbnails.merge(t);
        }
        if let Some(i) = other.images {
            self.images = self.images.merge(i);
        }
        if let Some(t) = other.theme {
            self.theme = self.theme.merge(t);
        }
        if let Some(f) = other.font {
            self.font = self.font.merge(f);
        }
        if let Some(p) = other.processing {
            self.processing = self.processing.merge(p);
        }
        self
    }
}

/// Parallel processing settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ProcessingConfig {
    /// Maximum number of parallel image processing workers.
    /// When absent or null, defaults to the number of CPU cores.
    /// Values larger than the core count are clamped down.
    pub max_processes: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialProcessingConfig {
    pub max_processes: Option<usize>,
}

impl ProcessingConfig {
    pub fn merge(mut self, other: PartialProcessingConfig) -> Self {
        if other.max_processes.is_some() {
            self.max_processes = other.max_processes;
        }
        self
    }
}

/// Font category — determines fallback fonts in the CSS font stack.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FontType {
    #[default]
    Sans,
    Serif,
}

/// Font configuration for the site.
///
/// By default, the font is loaded from Google Fonts via a `<link>` tag.
/// Set `source` to a local font file path (relative to site root) to use
/// a self-hosted font instead — this generates a `@font-face` declaration
/// and skips the Google Fonts request entirely.
///
/// ```toml
/// # Google Fonts (default)
/// [font]
/// font = "Space Grotesk"
/// weight = "600"
/// font_type = "sans"
///
/// # Local font (put the file in your assets directory)
/// [font]
/// font = "My Custom Font"
/// weight = "400"
/// font_type = "sans"
/// source = "fonts/MyFont.woff2"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct FontConfig {
    /// Font family name (Google Fonts family or custom name for local fonts).
    pub font: String,
    /// Font weight to load (e.g. `"600"`).
    pub weight: String,
    /// Font category: `"sans"` or `"serif"` — determines fallback fonts.
    pub font_type: FontType,
    /// Path to a local font file, relative to the site root (e.g. `"fonts/MyFont.woff2"`).
    /// When set, generates `@font-face` CSS instead of loading from Google Fonts.
    /// The file should be placed in the assets directory so it gets copied to the output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialFontConfig {
    pub font: Option<String>,
    pub weight: Option<String>,
    pub font_type: Option<FontType>,
    pub source: Option<String>,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            font: "Space Grotesk".to_string(),
            weight: "600".to_string(),
            font_type: FontType::Sans,
            source: None,
        }
    }
}

impl FontConfig {
    pub fn merge(mut self, other: PartialFontConfig) -> Self {
        if let Some(f) = other.font {
            self.font = f;
        }
        if let Some(w) = other.weight {
            self.weight = w;
        }
        if let Some(t) = other.font_type {
            self.font_type = t;
        }
        if other.source.is_some() {
            self.source = other.source;
        }
        self
    }

    /// Whether this font is loaded from a local file (vs. Google Fonts).
    pub fn is_local(&self) -> bool {
        self.source.is_some()
    }

    /// Google Fonts stylesheet URL for use in a `<link>` element.
    /// Returns `None` for local fonts.
    pub fn stylesheet_url(&self) -> Option<String> {
        if self.is_local() {
            return None;
        }
        let family = self.font.replace(' ', "+");
        Some(format!(
            "https://fonts.googleapis.com/css2?family={}:wght@{}&display=swap",
            family, self.weight
        ))
    }

    /// Generate `@font-face` CSS for a local font.
    /// Returns `None` for Google Fonts.
    pub fn font_face_css(&self) -> Option<String> {
        let src = self.source.as_ref()?;
        let format = font_format_from_extension(src);
        Some(format!(
            r#"@font-face {{
    font-family: "{}";
    src: url("/{}") format("{}");
    font-weight: {};
    font-display: swap;
}}"#,
            self.font, src, format, self.weight
        ))
    }

    /// CSS `font-family` value with fallbacks based on `font_type`.
    pub fn font_family_css(&self) -> String {
        let fallbacks = match self.font_type {
            FontType::Serif => r#"Georgia, "Times New Roman", serif"#,
            FontType::Sans => "Helvetica, Verdana, sans-serif",
        };
        format!(r#""{}", {}"#, self.font, fallbacks)
    }
}

/// Determine the CSS font format string from a file extension.
fn font_format_from_extension(path: &str) -> &'static str {
    match path.rsplit('.').next().map(|e| e.to_lowercase()).as_deref() {
        Some("woff2") => "woff2",
        Some("woff") => "woff",
        Some("ttf") => "truetype",
        Some("otf") => "opentype",
        _ => "woff2", // sensible default
    }
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ThumbnailsConfig {
    /// Aspect ratio as `[width, height]`, e.g. `[4, 5]` for portrait thumbnails.
    pub aspect_ratio: [u32; 2],
    /// Thumbnail short-edge size in pixels.
    pub size: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialThumbnailsConfig {
    pub aspect_ratio: Option<[u32; 2]>,
    pub size: Option<u32>,
}

impl ThumbnailsConfig {
    pub fn merge(mut self, other: PartialThumbnailsConfig) -> Self {
        if let Some(ar) = other.aspect_ratio {
            self.aspect_ratio = ar;
        }
        if let Some(s) = other.size {
            self.size = s;
        }
        self
    }
}

impl Default for ThumbnailsConfig {
    fn default() -> Self {
        Self {
            aspect_ratio: [4, 5],
            size: 400,
        }
    }
}

/// Responsive image generation settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ImagesConfig {
    /// Pixel widths (longer edge) to generate for responsive `<picture>` elements.
    pub sizes: Vec<u32>,
    /// AVIF encoding quality (0 = worst, 100 = best).
    pub quality: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialImagesConfig {
    pub sizes: Option<Vec<u32>>,
    pub quality: Option<u32>,
}

impl ImagesConfig {
    pub fn merge(mut self, other: PartialImagesConfig) -> Self {
        if let Some(s) = other.sizes {
            self.sizes = s;
        }
        if let Some(q) = other.quality {
            self.quality = q;
        }
        self
    }
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ClampSize {
    /// Preferred/fluid value, typically viewport-relative (e.g. `"3vw"`).
    pub size: String,
    /// Minimum bound (e.g. `"1rem"`).
    pub min: String,
    /// Maximum bound (e.g. `"2.5rem"`).
    pub max: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialClampSize {
    pub size: Option<String>,
    pub min: Option<String>,
    pub max: Option<String>,
}

impl ClampSize {
    pub fn merge(mut self, other: PartialClampSize) -> Self {
        if let Some(s) = other.size {
            self.size = s;
        }
        if let Some(m) = other.min {
            self.min = m;
        }
        if let Some(m) = other.max {
            self.max = m;
        }
        self
    }
}

impl ClampSize {
    /// Render as a CSS `clamp()` expression.
    pub fn to_css(&self) -> String {
        format!("clamp({}, {}, {})", self.min, self.size, self.max)
    }
}

/// Theme/layout settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    /// Horizontal mat around images (left/right). See docs/dev/photo-page-layout.md.
    pub mat_x: ClampSize,
    /// Vertical mat around images (top/bottom). See docs/dev/photo-page-layout.md.
    pub mat_y: ClampSize,
    /// Gap between thumbnails in both album and image grids (CSS value).
    pub thumbnail_gap: String,
    /// Padding around the thumbnail grid container (CSS value).
    pub grid_padding: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialThemeConfig {
    pub mat_x: Option<PartialClampSize>,
    pub mat_y: Option<PartialClampSize>,
    pub thumbnail_gap: Option<String>,
    pub grid_padding: Option<String>,
}

impl ThemeConfig {
    pub fn merge(mut self, other: PartialThemeConfig) -> Self {
        if let Some(x) = other.mat_x {
            self.mat_x = self.mat_x.merge(x);
        }
        if let Some(y) = other.mat_y {
            self.mat_y = self.mat_y.merge(y);
        }
        if let Some(g) = other.thumbnail_gap {
            self.thumbnail_gap = g;
        }
        if let Some(p) = other.grid_padding {
            self.grid_padding = p;
        }
        self
    }
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            mat_x: ClampSize {
                size: "3vw".to_string(),
                min: "1rem".to_string(),
                max: "2.5rem".to_string(),
            },
            mat_y: ClampSize {
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ColorConfig {
    /// Light mode color scheme.
    pub light: ColorScheme,
    /// Dark mode color scheme.
    pub dark: ColorScheme,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialColorConfig {
    pub light: Option<PartialColorScheme>,
    pub dark: Option<PartialColorScheme>,
}

impl ColorConfig {
    pub fn merge(mut self, other: PartialColorConfig) -> Self {
        if let Some(l) = other.light {
            self.light = self.light.merge(l);
        }
        if let Some(d) = other.dark {
            self.dark = self.dark.merge(d);
        }
        self
    }
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    /// Separator color (header bar underline, nav menu divider).
    pub separator: String,
    /// Link color.
    pub link: String,
    /// Link hover color.
    pub link_hover: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialColorScheme {
    pub background: Option<String>,
    pub text: Option<String>,
    pub text_muted: Option<String>,
    pub border: Option<String>,
    pub separator: Option<String>,
    pub link: Option<String>,
    pub link_hover: Option<String>,
}

impl ColorScheme {
    pub fn merge(mut self, other: PartialColorScheme) -> Self {
        if let Some(v) = other.background {
            self.background = v;
        }
        if let Some(v) = other.text {
            self.text = v;
        }
        if let Some(v) = other.text_muted {
            self.text_muted = v;
        }
        if let Some(v) = other.border {
            self.border = v;
        }
        if let Some(v) = other.separator {
            self.separator = v;
        }
        if let Some(v) = other.link {
            self.link = v;
        }
        if let Some(v) = other.link_hover {
            self.link_hover = v;
        }
        self
    }
}

impl ColorScheme {
    pub fn default_light() -> Self {
        Self {
            background: "#ffffff".to_string(),
            text: "#111111".to_string(),
            text_muted: "#666666".to_string(),
            border: "#e0e0e0".to_string(),
            separator: "#e0e0e0".to_string(),
            link: "#333333".to_string(),
            link_hover: "#000000".to_string(),
        }
    }

    pub fn default_dark() -> Self {
        Self {
            background: "#000000".to_string(),
            text: "#fafafa".to_string(),
            text_muted: "#999999".to_string(),
            border: "#333333".to_string(),
            separator: "#333333".to_string(),
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

/// Load a partial, validated config from `config.toml`.
///
/// Returns `Ok(None)` if no `config.toml` exists.
/// Returns `Err` if the file exists but contains unknown keys or invalid values.
pub fn load_partial_config(path: &Path) -> Result<Option<PartialSiteConfig>, ConfigError> {
    let config_path = path.join("config.toml");
    if !config_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&config_path)?;
    let partial: PartialSiteConfig = toml::from_str(&content)?;
    Ok(Some(partial))
}

/// Load config from `config.toml` in the given directory and merge onto defaults.
pub fn load_config(root: &Path) -> Result<SiteConfig, ConfigError> {
    let base = SiteConfig::default();
    let partial = load_partial_config(root)?;
    if let Some(p) = partial {
        let merged = base.merge(p);
        merged.validate()?;
        Ok(merged)
    } else {
        Ok(base)
    }
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

# Site title shown in breadcrumbs and the browser tab for the home page.
site_title = "Gallery"

# Directory for static assets (favicon, fonts, etc.), relative to content root.
# Contents are copied verbatim to the output root during generation.
# If the directory doesn't exist, it is silently skipped.
assets_dir = "assets"

# Stem of the site description file in the content root.
# If site.md or site.txt exists, its content is rendered on the index page.
# site_description_file = "site"

# ---------------------------------------------------------------------------
# Thumbnail generation
# ---------------------------------------------------------------------------
[thumbnails]
# Aspect ratio as [width, height] for thumbnail crops.
# Common choices: [1, 1] for square, [4, 5] for portrait, [3, 2] for landscape.
aspect_ratio = [4, 5]

# Short-edge size in pixels for generated thumbnails.
size = 400

# ---------------------------------------------------------------------------
# Responsive image generation
# ---------------------------------------------------------------------------
[images]
# Pixel widths (longer edge) to generate for responsive <picture> elements.
sizes = [800, 1400, 2080]

# AVIF encoding quality (0 = worst, 100 = best).
quality = 90

# ---------------------------------------------------------------------------
# Theme / layout
# ---------------------------------------------------------------------------
[theme]
# Gap between thumbnails in album and image grids (CSS value).
thumbnail_gap = "1rem"

# Padding around the thumbnail grid container (CSS value).
grid_padding = "2rem"

# Horizontal mat around images, as CSS clamp(min, size, max).
# See docs/dev/photo-page-layout.md for the layout spec.
[theme.mat_x]
size = "3vw"
min = "1rem"
max = "2.5rem"

# Vertical mat around images, as CSS clamp(min, size, max).
[theme.mat_y]
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
separator = "#e0e0e0"     # Header underline, nav menu divider
link = "#333333"
link_hover = "#000000"

# ---------------------------------------------------------------------------
# Colors - Dark mode (prefers-color-scheme: dark)
# ---------------------------------------------------------------------------
[colors.dark]
background = "#000000"
text = "#fafafa"
text_muted = "#999999"
border = "#333333"
separator = "#333333"     # Header underline, nav menu divider
link = "#cccccc"
link_hover = "#ffffff"

# ---------------------------------------------------------------------------
# Font
# ---------------------------------------------------------------------------
[font]
# Google Fonts family name.
font = "Space Grotesk"

# Font weight to load from Google Fonts.
weight = "600"

# Font category: "sans" or "serif". Determines fallback fonts in the CSS stack.
# sans  -> Helvetica, Verdana, sans-serif
# serif -> Georgia, "Times New Roman", serif
font_type = "sans"

# Local font file path, relative to the site root (e.g. "fonts/MyFont.woff2").
# When set, generates @font-face CSS instead of loading from Google Fonts.
# Place the font file in your assets directory so it gets copied to the output.
# Supported formats: .woff2, .woff, .ttf, .otf
# source = "fonts/MyFont.woff2"

# ---------------------------------------------------------------------------
# Processing
# ---------------------------------------------------------------------------
[processing]
# Maximum parallel image-processing workers.
# Omit or comment out to auto-detect (= number of CPU cores).
# max_processes = 4

# ---------------------------------------------------------------------------
# Custom CSS & HTML Snippets
# ---------------------------------------------------------------------------
# Drop any of these files into your assets/ directory to inject custom content.
# No configuration needed — the files are detected automatically.
#
#   assets/custom.css    → <link rel="stylesheet"> after main styles (CSS overrides)
#   assets/head.html     → raw HTML at the end of <head> (analytics, meta tags)
#   assets/body-end.html → raw HTML before </body> (tracking scripts, widgets)
"##
}

/// Generate CSS custom properties from color config.
///
/// These `generate_*_css()` functions produce `:root { … }` blocks that are
/// prepended to the inline `<style>` in every page. The Google Font is loaded
/// separately via a `<link>` tag (see `FontConfig::stylesheet_url` and
/// `base_document` in generate.rs). Variables defined here are consumed by
/// `static/style.css`; do not redefine them there.
pub fn generate_color_css(colors: &ColorConfig) -> String {
    format!(
        r#":root {{
    --color-bg: {light_bg};
    --color-text: {light_text};
    --color-text-muted: {light_text_muted};
    --color-border: {light_border};
    --color-link: {light_link};
    --color-link-hover: {light_link_hover};
    --color-separator: {light_separator};
}}

@media (prefers-color-scheme: dark) {{
    :root {{
        --color-bg: {dark_bg};
        --color-text: {dark_text};
        --color-text-muted: {dark_text_muted};
        --color-border: {dark_border};
        --color-link: {dark_link};
        --color-link-hover: {dark_link_hover};
        --color-separator: {dark_separator};
    }}
}}"#,
        light_bg = colors.light.background,
        light_text = colors.light.text,
        light_text_muted = colors.light.text_muted,
        light_border = colors.light.border,
        light_separator = colors.light.separator,
        light_link = colors.light.link,
        light_link_hover = colors.light.link_hover,
        dark_bg = colors.dark.background,
        dark_text = colors.dark.text,
        dark_text_muted = colors.dark.text_muted,
        dark_border = colors.dark.border,
        dark_separator = colors.dark.separator,
        dark_link = colors.dark.link,
        dark_link_hover = colors.dark.link_hover,
    )
}

/// Generate CSS custom properties from theme config.
pub fn generate_theme_css(theme: &ThemeConfig) -> String {
    format!(
        r#":root {{
    --mat-x: {mat_x};
    --mat-y: {mat_y};
    --thumbnail-gap: {thumbnail_gap};
    --grid-padding: {grid_padding};
}}"#,
        mat_x = theme.mat_x.to_css(),
        mat_y = theme.mat_y.to_css(),
        thumbnail_gap = theme.thumbnail_gap,
        grid_padding = theme.grid_padding,
    )
}

/// Generate CSS custom properties from font config.
///
/// For local fonts, also includes the `@font-face` declaration.
pub fn generate_font_css(font: &FontConfig) -> String {
    let vars = format!(
        r#":root {{
    --font-family: {family};
    --font-weight: {weight};
}}"#,
        family = font.font_family_css(),
        weight = font.weight,
    );
    match font.font_face_css() {
        Some(face) => format!("{}\n\n{}", face, vars),
        None => vars,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_config_has_colors() {
        let config = SiteConfig::default();
        assert_eq!(config.colors.light.background, "#ffffff");
        assert_eq!(config.colors.dark.background, "#000000");
    }

    #[test]
    fn default_config_has_content_root() {
        let config = SiteConfig::default();
        assert_eq!(config.content_root, "content");
    }

    #[test]
    fn default_config_has_site_title() {
        let config = SiteConfig::default();
        assert_eq!(config.site_title, "Gallery");
    }

    #[test]
    fn parse_custom_site_title() {
        let toml = r#"site_title = "My Portfolio""#;
        let partial: PartialSiteConfig = toml::from_str(toml).unwrap();
        let config = SiteConfig::default().merge(partial);
        assert_eq!(config.site_title, "My Portfolio");
    }

    #[test]
    fn default_config_has_image_settings() {
        let config = SiteConfig::default();
        assert_eq!(config.thumbnails.aspect_ratio, [4, 5]);
        assert_eq!(config.images.sizes, vec![800, 1400, 2080]);
        assert_eq!(config.images.quality, 90);
        assert_eq!(config.theme.mat_x.to_css(), "clamp(1rem, 3vw, 2.5rem)");
        assert_eq!(config.theme.mat_y.to_css(), "clamp(2rem, 6vw, 5rem)");
    }

    #[test]
    fn parse_partial_config() {
        let toml = r##"
[colors.light]
background = "#fafafa"
"##;
        let partial: PartialSiteConfig = toml::from_str(toml).unwrap();
        let config = SiteConfig::default().merge(partial);

        // Overridden value
        assert_eq!(config.colors.light.background, "#fafafa");
        // Default values preserved
        assert_eq!(config.colors.light.text, "#111111");
        assert_eq!(config.colors.dark.background, "#000000");
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
        let partial: PartialSiteConfig = toml::from_str(toml).unwrap();
        let config = SiteConfig::default().merge(partial);

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
        assert_eq!(config.colors.dark.background, "#000000");
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
        assert_eq!(config.colors.dark.background, "#000000");
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

    #[test]
    fn load_config_unknown_keys_is_error() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("config.toml");

        // "unknown_key" is not a valid field
        fs::write(
            &config_path,
            r#"
            unknown_key = "foo"
            "#,
        )
        .unwrap();

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
    fn generate_theme_css_includes_mat_variables() {
        let theme = ThemeConfig::default();
        let css = generate_theme_css(&theme);
        assert!(css.contains("--mat-x: clamp(1rem, 3vw, 2.5rem)"));
        assert!(css.contains("--mat-y: clamp(2rem, 6vw, 5rem)"));
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
        let partial: PartialSiteConfig = toml::from_str(toml).unwrap();
        let config = SiteConfig::default().merge(partial);
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
    // merge_toml tests - REMOVED (function removed)
    // =========================================================================

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
    // load_partial_config / merge tests
    // =========================================================================

    #[test]
    fn load_partial_config_returns_none_when_no_file() {
        let tmp = TempDir::new().unwrap();
        let result = load_partial_config(tmp.path()).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn load_partial_config_returns_value_when_file_exists() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("config.toml"),
            r#"
[images]
quality = 85
"#,
        )
        .unwrap();

        let result = load_partial_config(tmp.path()).unwrap();
        assert!(result.is_some());
        let partial = result.unwrap();
        assert_eq!(partial.images.unwrap().quality, Some(85));
    }

    #[test]
    fn merge_with_no_overlay() {
        let base = SiteConfig::default();
        let config = base.merge(PartialSiteConfig::default());
        assert_eq!(config.images.quality, 90);
        assert_eq!(config.colors.light.background, "#ffffff");
    }

    #[test]
    fn merge_with_overlay() {
        let base = SiteConfig::default();
        let toml = r#"
[images]
quality = 70
"#;
        let partial: PartialSiteConfig = toml::from_str(toml).unwrap();
        let config = base.merge(partial);
        assert_eq!(config.images.quality, 70);
        // Other fields preserved from defaults
        assert_eq!(config.images.sizes, vec![800, 1400, 2080]);
    }

    #[test]
    fn load_config_validates_after_merge() {
        let tmp = TempDir::new().unwrap();
        // Create config with invalid value
        fs::write(
            tmp.path().join("config.toml"),
            r#"
[images]
quality = 200
"#,
        )
        .unwrap();

        // load_config should fail validation
        let result = load_config(tmp.path());
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
        assert_eq!(config.colors.dark.background, "#000000");
        assert_eq!(config.theme.thumbnail_gap, "1rem");
    }

    #[test]
    fn stock_config_toml_contains_all_sections() {
        let content = stock_config_toml();
        assert!(content.contains("[thumbnails]"));
        assert!(content.contains("[images]"));
        assert!(content.contains("[theme]"));
        assert!(content.contains("[theme.mat_x]"));
        assert!(content.contains("[theme.mat_y]"));
        assert!(content.contains("[colors.light]"));
        assert!(content.contains("[colors.dark]"));
        assert!(content.contains("[processing]"));
    }

    #[test]
    fn stock_defaults_equivalent_to_default_trait() {
        // We removed stock_defaults_value, but we can test that Default trait works
        let config = SiteConfig::default();
        assert_eq!(config.images.quality, 90);
    }

    // =========================================================================
    // Partial nested merge tests — verify unset fields are preserved
    // =========================================================================

    #[test]
    fn merge_partial_theme_mat_x_only() {
        let partial: PartialSiteConfig = toml::from_str(
            r#"
            [theme.mat_x]
            size = "5vw"
        "#,
        )
        .unwrap();
        let config = SiteConfig::default().merge(partial);

        // Overridden
        assert_eq!(config.theme.mat_x.size, "5vw");
        // Preserved from defaults
        assert_eq!(config.theme.mat_x.min, "1rem");
        assert_eq!(config.theme.mat_x.max, "2.5rem");
        // mat_y entirely untouched
        assert_eq!(config.theme.mat_y.size, "6vw");
        assert_eq!(config.theme.mat_y.min, "2rem");
        assert_eq!(config.theme.mat_y.max, "5rem");
        // Other theme fields untouched
        assert_eq!(config.theme.thumbnail_gap, "1rem");
        assert_eq!(config.theme.grid_padding, "2rem");
    }

    #[test]
    fn merge_partial_colors_light_only() {
        let partial: PartialSiteConfig = toml::from_str(
            r##"
            [colors.light]
            background = "#fafafa"
            text = "#222222"
        "##,
        )
        .unwrap();
        let config = SiteConfig::default().merge(partial);

        // Overridden
        assert_eq!(config.colors.light.background, "#fafafa");
        assert_eq!(config.colors.light.text, "#222222");
        // Light defaults preserved for unset fields
        assert_eq!(config.colors.light.text_muted, "#666666");
        assert_eq!(config.colors.light.border, "#e0e0e0");
        assert_eq!(config.colors.light.link, "#333333");
        assert_eq!(config.colors.light.link_hover, "#000000");
        // Dark entirely untouched
        assert_eq!(config.colors.dark.background, "#000000");
        assert_eq!(config.colors.dark.text, "#fafafa");
    }

    #[test]
    fn merge_partial_font_weight_only() {
        let partial: PartialSiteConfig = toml::from_str(
            r#"
            [font]
            weight = "300"
        "#,
        )
        .unwrap();
        let config = SiteConfig::default().merge(partial);

        assert_eq!(config.font.weight, "300");
        assert_eq!(config.font.font, "Space Grotesk");
        assert_eq!(config.font.font_type, FontType::Sans);
    }

    #[test]
    fn merge_multiple_sections_independently() {
        let partial: PartialSiteConfig = toml::from_str(
            r##"
            [colors.dark]
            background = "#1a1a1a"

            [font]
            font = "Lora"
            font_type = "serif"
        "##,
        )
        .unwrap();
        let config = SiteConfig::default().merge(partial);

        // Each section merged independently
        assert_eq!(config.colors.dark.background, "#1a1a1a");
        assert_eq!(config.colors.dark.text, "#fafafa");
        assert_eq!(config.colors.light.background, "#ffffff");

        assert_eq!(config.font.font, "Lora");
        assert_eq!(config.font.font_type, FontType::Serif);
        assert_eq!(config.font.weight, "600"); // preserved

        // Sections not mentioned at all → full defaults
        assert_eq!(config.images.quality, 90);
        assert_eq!(config.thumbnails.aspect_ratio, [4, 5]);
        assert_eq!(config.theme.mat_x.size, "3vw");
    }

    // =========================================================================
    // Assets directory tests
    // =========================================================================

    #[test]
    fn default_assets_dir() {
        let config = SiteConfig::default();
        assert_eq!(config.assets_dir, "assets");
    }

    #[test]
    fn parse_custom_assets_dir() {
        let toml = r#"assets_dir = "site-assets""#;
        let partial: PartialSiteConfig = toml::from_str(toml).unwrap();
        let config = SiteConfig::default().merge(partial);
        assert_eq!(config.assets_dir, "site-assets");
    }

    #[test]
    fn merge_preserves_default_assets_dir() {
        let partial: PartialSiteConfig = toml::from_str("[images]\nquality = 70\n").unwrap();
        let config = SiteConfig::default().merge(partial);
        assert_eq!(config.assets_dir, "assets");
    }

    // =========================================================================
    // Site description file tests
    // =========================================================================

    #[test]
    fn default_site_description_file() {
        let config = SiteConfig::default();
        assert_eq!(config.site_description_file, "site");
    }

    #[test]
    fn parse_custom_site_description_file() {
        let toml = r#"site_description_file = "intro""#;
        let partial: PartialSiteConfig = toml::from_str(toml).unwrap();
        let config = SiteConfig::default().merge(partial);
        assert_eq!(config.site_description_file, "intro");
    }

    #[test]
    fn merge_preserves_default_site_description_file() {
        let partial: PartialSiteConfig = toml::from_str("[images]\nquality = 70\n").unwrap();
        let config = SiteConfig::default().merge(partial);
        assert_eq!(config.site_description_file, "site");
    }

    // =========================================================================
    // Local font tests
    // =========================================================================

    #[test]
    fn default_font_is_google() {
        let config = FontConfig::default();
        assert!(!config.is_local());
        assert!(config.stylesheet_url().is_some());
        assert!(config.font_face_css().is_none());
    }

    #[test]
    fn local_font_has_no_stylesheet_url() {
        let config = FontConfig {
            source: Some("fonts/MyFont.woff2".to_string()),
            ..FontConfig::default()
        };
        assert!(config.is_local());
        assert!(config.stylesheet_url().is_none());
    }

    #[test]
    fn local_font_generates_font_face_css() {
        let config = FontConfig {
            font: "My Custom Font".to_string(),
            weight: "400".to_string(),
            font_type: FontType::Sans,
            source: Some("fonts/MyFont.woff2".to_string()),
        };
        let css = config.font_face_css().unwrap();
        assert!(css.contains("@font-face"));
        assert!(css.contains(r#"font-family: "My Custom Font""#));
        assert!(css.contains(r#"url("/fonts/MyFont.woff2")"#));
        assert!(css.contains(r#"format("woff2")"#));
        assert!(css.contains("font-weight: 400"));
        assert!(css.contains("font-display: swap"));
    }

    #[test]
    fn parse_font_with_source() {
        let toml = r#"
[font]
font = "My Font"
weight = "400"
source = "fonts/myfont.woff2"
"#;
        let partial: PartialSiteConfig = toml::from_str(toml).unwrap();
        let config = SiteConfig::default().merge(partial);
        assert_eq!(config.font.font, "My Font");
        assert_eq!(config.font.source.as_deref(), Some("fonts/myfont.woff2"));
        assert!(config.font.is_local());
    }

    #[test]
    fn merge_font_source_preserves_other_fields() {
        let partial: PartialSiteConfig = toml::from_str(
            r#"
[font]
source = "fonts/custom.woff2"
"#,
        )
        .unwrap();
        let config = SiteConfig::default().merge(partial);
        assert_eq!(config.font.font, "Space Grotesk"); // default preserved
        assert_eq!(config.font.weight, "600"); // default preserved
        assert_eq!(config.font.source.as_deref(), Some("fonts/custom.woff2"));
    }

    #[test]
    fn font_format_detection() {
        assert_eq!(font_format_from_extension("font.woff2"), "woff2");
        assert_eq!(font_format_from_extension("font.woff"), "woff");
        assert_eq!(font_format_from_extension("font.ttf"), "truetype");
        assert_eq!(font_format_from_extension("font.otf"), "opentype");
        assert_eq!(font_format_from_extension("font.unknown"), "woff2");
    }

    #[test]
    fn generate_font_css_includes_font_face_for_local() {
        let font = FontConfig {
            font: "Local Font".to_string(),
            weight: "700".to_string(),
            font_type: FontType::Serif,
            source: Some("fonts/local.woff2".to_string()),
        };
        let css = generate_font_css(&font);
        assert!(css.contains("@font-face"));
        assert!(css.contains("--font-family:"));
        assert!(css.contains("--font-weight: 700"));
    }

    #[test]
    fn generate_font_css_no_font_face_for_google() {
        let font = FontConfig::default();
        let css = generate_font_css(&font);
        assert!(!css.contains("@font-face"));
        assert!(css.contains("--font-family:"));
    }
}
