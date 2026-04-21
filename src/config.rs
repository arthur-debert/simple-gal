//! Site configuration module.
//!
//! `SiteConfig` is a [confique][confique::Config] struct: defaults live as
//! `#[config(default = ...)]` annotations on the fields, sparse loading +
//! deep merge are handled by confique's generated `Layer` type, and the
//! [`simple-gal config`][crate] CLI subcommand group is wired through
//! [clapfig][clapfig].
//!
//! ## Per-directory cascade
//!
//! Site config is hierarchical: a `config.toml` may live in the content
//! root, in any album group, and in any album directory. The scan stage
//! walks the tree and merges every `config.toml` it finds onto its parent's
//! resolved config, so each album sees its own combined view (root → group
//! → gallery). The cascade machinery lives in `scan.rs`; this module owns
//! the type, defaults, validation, and CSS-emitting helpers consumed by
//! `generate.rs`.
//!
//! ## Configuration shape
//!
//! ```toml
//! site_title = "Gallery"
//! assets_dir = "assets"
//!
//! [thumbnails]
//! aspect_ratio = [4, 5]
//! size = 400
//!
//! [full_index]
//! generates = false
//! show_link = false
//! thumb_ratio = [4, 5]
//! thumb_size = 400
//! thumb_gap = "0.2rem"
//!
//! [images]
//! sizes = [800, 1400, 2080]
//! quality = 90
//!
//! [theme]
//! thumbnail_gap = "0.2rem"
//! grid_padding = "2rem"
//!
//! [theme.mat_x]
//! size = "3vw"
//! min  = "1rem"
//! max  = "2.5rem"
//!
//! [theme.mat_y]
//! size = "6vw"
//! min  = "2rem"
//! max  = "5rem"
//!
//! [colors.light]
//! background = "#ffffff"
//! # ...
//!
//! [colors.dark]
//! background = "#000000"
//! # ...
//!
//! [font]
//! font = "Noto Sans"
//! weight = "600"
//! font_type = "sans"
//!
//! [processing]
//! # max_processes = 4   # omit for auto-detect
//! ```
//!
//! Run `simple-gal config gen` to print a documented template derived
//! directly from this struct.

use confique::Config;
use confique::Layer;
use confique::meta::Meta;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// A TOML parse failure on a specific config file.
    ///
    /// Carries the originating path and the full file contents so callers
    /// (e.g. the CLI in `main.rs`) can hand the error to clapfig's renderer
    /// for a snippet + caret view instead of showing a bare parser message.
    #[error("failed to parse {}: {source}", path.display())]
    Toml {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
        source_text: String,
    },
    /// confique's deserialize/build error from converting a merged layer
    /// into the typed `SiteConfig`. Distinct from `Toml` because it can
    /// fire on a layer that came from multiple files.
    #[error("config error: {0}")]
    Confique(#[from] confique::Error),
    #[error("Config validation error: {0}")]
    Validation(String),
}

impl ConfigError {
    /// Convert a config error into the richer `clapfig::error::ClapfigError`
    /// representation when possible, so the CLI can render it through
    /// clapfig's plain/rich (miette) renderers. Returns `None` for error
    /// kinds that don't carry source-file context (IO failures, range
    /// validation failures).
    pub fn to_clapfig_error(&self) -> Option<clapfig::error::ClapfigError> {
        match self {
            ConfigError::Toml {
                path,
                source,
                source_text,
            } => Some(clapfig::error::ClapfigError::ParseError {
                path: path.clone(),
                source: source.clone(),
                source_text: Some(Arc::from(source_text.as_str())),
            }),
            _ => None,
        }
    }
}

// =============================================================================
// SiteConfig — top level
// =============================================================================

/// Site configuration loaded from `config.toml`.
///
/// All fields have sensible defaults. User config files need only specify
/// the values they want to override. Unknown keys are rejected.
//
// Note: `Deserialize` is implemented manually below so that any caller
// reading a `SiteConfig` from JSON or TOML — including the manifest reader
// in `process.rs` — gets the same sparse-tolerant + default-merge semantics
// as `load_config`. Kept as a regular comment so it doesn't leak into the
// schema/template doc strings.
#[derive(Config, Debug, Clone, Serialize, PartialEq)]
#[config(layer_attr(derive(Clone)))]
#[config(layer_attr(serde(deny_unknown_fields)))]
pub struct SiteConfig {
    /// Site title used in breadcrumbs and the browser tab for the home page.
    #[config(default = "Gallery")]
    pub site_title: String,

    /// Public origin of the deployed site (e.g. `"https://gallery.example.com"`),
    /// with no trailing slash. When set, the generator emits Open Graph meta
    /// tags on gallery-list, album, and image pages so chat apps (WhatsApp,
    /// iMessage, Slack, Discord) render link previews with an image, title,
    /// and breadcrumb description. When unset, no OG tags are emitted — the
    /// site still works, it just won't produce rich link previews.
    pub base_url: Option<String>,

    /// Directory for static assets (favicon, fonts, etc.), relative to
    /// content root. Contents are copied verbatim to the output root during
    /// generation. If the directory doesn't exist, it is silently skipped.
    #[config(default = "assets")]
    pub assets_dir: String,

    /// Stem of the site description file in the content root (e.g. `site`
    /// → looks for `site.md` / `site.txt`). Rendered on the index page.
    #[config(default = "site")]
    pub site_description_file: String,

    /// Color schemes for light and dark modes.
    #[config(nested)]
    pub colors: ColorConfig,

    /// Thumbnail generation settings (aspect ratio, size).
    #[config(nested)]
    pub thumbnails: ThumbnailsConfig,

    /// Site-wide "All Photos" index settings.
    #[config(nested)]
    pub full_index: FullIndexConfig,

    /// Responsive image generation settings (sizes, quality).
    #[config(nested)]
    pub images: ImagesConfig,

    /// Theme / layout settings (mats, grid spacing).
    #[config(nested)]
    pub theme: ThemeConfig,

    /// Font configuration (Google Fonts or local font file).
    #[config(nested)]
    pub font: FontConfig,

    /// Parallel processing settings.
    #[config(nested)]
    pub processing: ProcessingConfig,

    /// Auto file-name index reindexing settings.
    #[config(nested)]
    pub auto_indexing: AutoIndexingConfig,
}

impl Default for SiteConfig {
    /// Construct a `SiteConfig` populated entirely from confique-declared
    /// defaults. Used by tests and by the scan stage to seed the cascade
    /// before any user `config.toml` is layered on.
    fn default() -> Self {
        let layer = <SiteConfig as Config>::Layer::default_values();
        SiteConfig::from_layer(layer).expect("confique defaults must satisfy the SiteConfig schema")
    }
}

impl<'de> Deserialize<'de> for SiteConfig {
    /// Custom deserialize that funnels any input (TOML config file, JSON
    /// manifest field, test fixture) through the same sparse-layer +
    /// fill-defaults pipeline `load_config` uses.
    ///
    /// Without this, missing fields on a directly-deserialized `SiteConfig`
    /// would be hard errors instead of falling through to confique-declared
    /// defaults — which would force every manifest writer (and every test
    /// fixture) to spell out every field explicitly.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let layer = SiteConfigLayer::deserialize(deserializer)?;
        let merged = layer.with_fallback(SiteConfigLayer::default_values());
        SiteConfig::from_layer(merged).map_err(serde::de::Error::custom)
    }
}

impl SiteConfig {
    /// Validate semantic constraints that confique's type system can't
    /// express: numeric ranges, non-empty arrays, and so on.
    ///
    /// Wired into clapfig's `.post_validate()` hook in [`load_config`] so
    /// every loaded config (CLI, cascade leaf, test fixture) runs the same
    /// checks.
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
        if self.full_index.thumb_ratio[0] == 0 || self.full_index.thumb_ratio[1] == 0 {
            return Err(ConfigError::Validation(
                "full_index.thumb_ratio values must be non-zero".into(),
            ));
        }
        if self.full_index.thumb_size == 0 {
            return Err(ConfigError::Validation(
                "full_index.thumb_size must be non-zero".into(),
            ));
        }
        if self.images.sizes.is_empty() {
            return Err(ConfigError::Validation(
                "images.sizes must not be empty".into(),
            ));
        }
        // Bound spacing/padding so the reindex step (10^spacing) and the
        // format-width allocation (padding chars) stay in sane ranges.
        // 10^9 is the largest step that fits in u32; padding beyond 12 is
        // already absurd and would just waste bytes.
        if self.auto_indexing.spacing > 9 {
            return Err(ConfigError::Validation(
                "auto_indexing.spacing must be 0-9 (step = 10^spacing)".into(),
            ));
        }
        if self.auto_indexing.padding > 12 {
            return Err(ConfigError::Validation(
                "auto_indexing.padding must be 0-12".into(),
            ));
        }
        Ok(())
    }
}

// =============================================================================
// Thumbnails
// =============================================================================

/// Thumbnail generation settings.
#[derive(Config, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[config(layer_attr(derive(Clone)))]
#[config(layer_attr(serde(deny_unknown_fields)))]
pub struct ThumbnailsConfig {
    /// Aspect ratio as `[width, height]`, e.g. `[4, 5]` for portrait.
    #[config(default = [4, 5])]
    pub aspect_ratio: [u32; 2],
    /// Thumbnail short-edge size in pixels.
    #[config(default = 400)]
    pub size: u32,
}

// =============================================================================
// Full index ("All Photos")
// =============================================================================

/// Settings for the site-wide "All Photos" index page.
///
/// When `generates` is true, the generate stage renders an extra page at
/// `/all-photos/` showing every image from every public album in a single
/// thumbnail grid. Thumbnails are generated at the ratio/size specified
/// here, independent of the regular per-album `[thumbnails]` settings.
#[derive(Config, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[config(layer_attr(derive(Clone)))]
#[config(layer_attr(serde(deny_unknown_fields)))]
pub struct FullIndexConfig {
    /// Whether the All Photos page is rendered.
    #[config(default = false)]
    pub generates: bool,
    /// Whether to add an "All Photos" item to the navigation menu.
    #[config(default = false)]
    pub show_link: bool,
    /// Aspect ratio `[width, height]` for full-index thumbnails.
    #[config(default = [4, 5])]
    pub thumb_ratio: [u32; 2],
    /// Short-edge size (in pixels) for full-index thumbnails.
    #[config(default = 400)]
    pub thumb_size: u32,
    /// Gap between thumbnails on the All Photos grid (CSS value).
    #[config(default = "0.2rem")]
    pub thumb_gap: String,
}

// =============================================================================
// Images
// =============================================================================

/// Responsive image generation settings.
#[derive(Config, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[config(layer_attr(derive(Clone)))]
#[config(layer_attr(serde(deny_unknown_fields)))]
pub struct ImagesConfig {
    /// Pixel widths (longer edge) to generate for responsive `<picture>`
    /// elements.
    #[config(default = [800, 1400, 2080])]
    pub sizes: Vec<u32>,
    /// AVIF encoding quality (0 = worst, 100 = best).
    #[config(default = 90)]
    pub quality: u32,
}

// =============================================================================
// Theme — mat_x and mat_y are split into distinct types so each side has
// its own confique-declared defaults (and shows up correctly in the schema).
// =============================================================================

/// Horizontal mat around images, expressed as `clamp(min, size, max)`.
///
/// - `size`: preferred/fluid value, typically viewport-relative (e.g. `3vw`)
/// - `min`: minimum bound (e.g. `1rem`)
/// - `max`: maximum bound (e.g. `2.5rem`)
#[derive(Config, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[config(layer_attr(derive(Clone)))]
#[config(layer_attr(serde(deny_unknown_fields)))]
pub struct MatX {
    /// Preferred/fluid value, typically viewport-relative.
    #[config(default = "3vw")]
    pub size: String,
    /// Minimum bound.
    #[config(default = "1rem")]
    pub min: String,
    /// Maximum bound.
    #[config(default = "2.5rem")]
    pub max: String,
}

/// Vertical mat around images, expressed as `clamp(min, size, max)`.
#[derive(Config, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[config(layer_attr(derive(Clone)))]
#[config(layer_attr(serde(deny_unknown_fields)))]
pub struct MatY {
    /// Preferred/fluid value, typically viewport-relative.
    #[config(default = "6vw")]
    pub size: String,
    /// Minimum bound.
    #[config(default = "2rem")]
    pub min: String,
    /// Maximum bound.
    #[config(default = "5rem")]
    pub max: String,
}

/// Render a `clamp(min, size, max)` CSS expression from the three parts.
fn clamp_to_css(size: &str, min: &str, max: &str) -> String {
    format!("clamp({}, {}, {})", min, size, max)
}

impl MatX {
    pub fn to_css(&self) -> String {
        clamp_to_css(&self.size, &self.min, &self.max)
    }
}

impl MatY {
    pub fn to_css(&self) -> String {
        clamp_to_css(&self.size, &self.min, &self.max)
    }
}

/// Theme / layout settings.
#[derive(Config, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[config(layer_attr(derive(Clone)))]
#[config(layer_attr(serde(deny_unknown_fields)))]
pub struct ThemeConfig {
    /// Horizontal mat around images. See `docs/dev/photo-page-layout.md`.
    #[config(nested)]
    pub mat_x: MatX,
    /// Vertical mat around images. See `docs/dev/photo-page-layout.md`.
    #[config(nested)]
    pub mat_y: MatY,
    /// Gap between thumbnails in both album and image grids (CSS value).
    #[config(default = "0.2rem")]
    pub thumbnail_gap: String,
    /// Padding around the thumbnail grid container (CSS value).
    #[config(default = "2rem")]
    pub grid_padding: String,
}

// =============================================================================
// Colors — light and dark are split into distinct types so each side has
// its own confique-declared defaults.
// =============================================================================

/// Color configuration for light and dark modes.
#[derive(Config, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[config(layer_attr(derive(Clone)))]
#[config(layer_attr(serde(deny_unknown_fields)))]
pub struct ColorConfig {
    /// Light mode color scheme.
    #[config(nested)]
    pub light: LightColors,
    /// Dark mode color scheme.
    #[config(nested)]
    pub dark: DarkColors,
}

/// Light-mode color scheme.
#[derive(Config, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[config(layer_attr(derive(Clone)))]
#[config(layer_attr(serde(deny_unknown_fields)))]
pub struct LightColors {
    /// Background color.
    #[config(default = "#ffffff")]
    pub background: String,
    /// Primary text color.
    #[config(default = "#111111")]
    pub text: String,
    /// Muted/secondary text color (nav menu, breadcrumbs, captions).
    #[config(default = "#666666")]
    pub text_muted: String,
    /// Border color.
    #[config(default = "#e0e0e0")]
    pub border: String,
    /// Separator color (header bar underline, nav menu divider).
    #[config(default = "#e0e0e0")]
    pub separator: String,
    /// Link color.
    #[config(default = "#333333")]
    pub link: String,
    /// Link hover color.
    #[config(default = "#000000")]
    pub link_hover: String,
}

/// Dark-mode color scheme.
#[derive(Config, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[config(layer_attr(derive(Clone)))]
#[config(layer_attr(serde(deny_unknown_fields)))]
pub struct DarkColors {
    /// Background color.
    #[config(default = "#000000")]
    pub background: String,
    /// Primary text color.
    #[config(default = "#fafafa")]
    pub text: String,
    /// Muted/secondary text color (nav menu, breadcrumbs, captions).
    #[config(default = "#999999")]
    pub text_muted: String,
    /// Border color.
    #[config(default = "#333333")]
    pub border: String,
    /// Separator color (header bar underline, nav menu divider).
    #[config(default = "#333333")]
    pub separator: String,
    /// Link color.
    #[config(default = "#cccccc")]
    pub link: String,
    /// Link hover color.
    #[config(default = "#ffffff")]
    pub link_hover: String,
}

// =============================================================================
// Font
// =============================================================================

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
/// Set `source` to a local font file path (relative to site root) to use a
/// self-hosted font instead — this generates a `@font-face` declaration
/// and skips the Google Fonts request entirely.
///
/// ```toml
/// # Google Fonts (default)
/// [font]
/// font = "Noto Sans"
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
#[derive(Config, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[config(layer_attr(derive(Clone)))]
#[config(layer_attr(serde(deny_unknown_fields)))]
pub struct FontConfig {
    /// Font family name (Google Fonts family or custom name for local fonts).
    #[config(default = "Noto Sans")]
    pub font: String,
    /// Font weight to load (e.g. `"600"`).
    #[config(default = "600")]
    pub weight: String,
    /// Font category: `"sans"` or `"serif"` — determines fallback fonts.
    #[config(default = "sans")]
    pub font_type: FontType,
    /// Path to a local font file, relative to the site root
    /// (e.g. `"fonts/MyFont.woff2"`). When set, generates `@font-face` CSS
    /// instead of loading from Google Fonts. The file should be placed in
    /// the assets directory so it gets copied to the output.
    pub source: Option<String>,
}

impl FontConfig {
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

    /// Generate `@font-face` CSS for a local font. Returns `None` for
    /// Google Fonts.
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

// =============================================================================
// Processing
// =============================================================================

/// Parallel processing settings.
#[derive(Config, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[config(layer_attr(derive(Clone)))]
#[config(layer_attr(serde(deny_unknown_fields)))]
pub struct ProcessingConfig {
    /// Maximum number of parallel image processing workers.
    /// When absent, defaults to the number of CPU cores.
    /// Values larger than the core count are clamped down.
    pub max_processes: Option<usize>,
}

// =============================================================================
// Auto-indexing
// =============================================================================

/// Auto file-name index reindexing settings.
///
/// `sync_source_files` is the single opt-in switch: when `true`, the build
/// pipeline runs the reindex walker over the source tree before scan,
/// renaming files in place. When `false` (default), the build leaves source
/// alone — users run `simple-gal reindex` manually when they want to tidy
/// filenames.
///
/// `spacing` and `padding` are the defaults used both by the auto hook and
/// by the `reindex` CLI command when the user doesn't supply explicit
/// `--spacing` / `--padding` flags.
///
/// Output prefix = `format!("{:0pad$}", n * 10^spacing)`:
/// - `spacing=0, padding=0` → `1, 2, 3, …`
/// - `spacing=1, padding=3` → `010, 020, 030, …` (the default)
/// - `spacing=2, padding=4` → `0100, 0200, 0300, …`
///
/// # Migration from the old enum-based `auto` field
///
/// Earlier releases carried an `auto` field that accepted one of `off` |
/// `source_only` | `export_only` | `both`. That design split user intent
/// from user effect:
///
/// - `source_only` and `both` had identical on-disk effect (rename source),
///   differing only in stated intent.
/// - `export_only` (rewrite output URLs without touching source) was never
///   implemented, and with ordinal-based URL naming it was tautological
///   anyway — output URLs don't carry source prefixes.
///
/// Collapsing the enum to a single boolean keeps the one meaningful knob
/// ("do I want source files renamed automatically?") and drops the three
/// modes that weren't pulling their weight. Old configs using `auto = "..."`
/// now fail with confique's unknown-field error, pointing users at this
/// field.
#[derive(Config, Debug, Clone, Serialize, Deserialize, PartialEq)]
#[config(layer_attr(derive(Clone)))]
#[config(layer_attr(serde(deny_unknown_fields)))]
pub struct AutoIndexingConfig {
    /// Whether the build pipeline renames source files before scan.
    /// `false` (default) leaves source alone; `true` runs the reindex
    /// walker over `--source` at build-start.
    #[config(default = false)]
    pub sync_source_files: bool,
    /// Step exponent: numbers are spaced by `10^spacing`.
    #[config(default = 1)]
    pub spacing: u32,
    /// Zero-pad numeric prefix to this width. `0` means no padding.
    #[config(default = 3)]
    pub padding: u32,
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

// =============================================================================
// Loading: per-file partial layer + per-directory cascade helpers
// =============================================================================

/// Confique-derived layer type for `SiteConfig`.
///
/// Every field is `Option<T>` so layers compose by overlay. The scan stage
/// produces one of these per `config.toml` it finds and folds them together
/// with [`with_fallback`][confique::Layer::with_fallback].
pub type SiteConfigLayer = <SiteConfig as Config>::Layer;

/// Load a single sparse `config.toml` from `dir` into a layer.
///
/// Returns `Ok(None)` if the file is absent — caller decides whether that's
/// an error. Returns `Err` for parse failures (with the original text
/// retained for snippet rendering) and for unknown-key violations enforced
/// by confique's strict deserializer.
pub fn load_layer(dir: &Path) -> Result<Option<SiteConfigLayer>, ConfigError> {
    let config_path = dir.join("config.toml");
    if !config_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&config_path)?;
    let layer: SiteConfigLayer = toml::from_str(&content).map_err(|e| ConfigError::Toml {
        path: config_path.clone(),
        source: Box::new(e),
        source_text: content,
    })?;
    Ok(Some(layer))
}

/// Load `config.toml` from `dir`, merge it onto confique defaults, and
/// validate.
///
/// Used at the root of the cascade and by tests that exercise the full
/// load → validate flow.
pub fn load_config(dir: &Path) -> Result<SiteConfig, ConfigError> {
    let user = load_layer(dir)?.unwrap_or_else(SiteConfigLayer::empty);
    let merged = user.with_fallback(SiteConfigLayer::default_values());
    let config = SiteConfig::from_layer(merged)?;
    config.validate()?;
    Ok(config)
}

/// Build a `SiteConfig` from a single layer, merging in confique defaults
/// for any unset fields. Validates before returning. Used by the scan
/// stage's per-directory cascade after layers have been folded together.
pub fn finalize_layer(layer: SiteConfigLayer) -> Result<SiteConfig, ConfigError> {
    let merged = layer.with_fallback(SiteConfigLayer::default_values());
    let config = SiteConfig::from_layer(merged)?;
    config.validate()?;
    Ok(config)
}

/// Static metadata for the `SiteConfig` schema. Re-exported so the
/// [`crate`] CLI can hand it to clapfig's schema-emitting subcommand
/// without having to depend on confique itself.
pub fn site_config_meta() -> &'static Meta {
    &<SiteConfig as Config>::META
}

// =============================================================================
// CSS generators (consumers of resolved config — not config plumbing)
// =============================================================================

/// Generate CSS custom properties from color config.
///
/// These `generate_*_css()` functions produce `:root { … }` blocks that are
/// prepended to the inline `<style>` in every page. The Google Font is
/// loaded separately via a `<link>` tag (see `FontConfig::stylesheet_url`
/// and `base_document` in `generate.rs`). Variables defined here are
/// consumed by `static/style.css`; do not redefine them there.
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

    fn write_config(dir: &Path, body: &str) {
        fs::write(dir.join("config.toml"), body).unwrap();
    }

    // ----- defaults -----

    #[test]
    fn default_config_has_colors() {
        let config = SiteConfig::default();
        assert_eq!(config.colors.light.background, "#ffffff");
        assert_eq!(config.colors.dark.background, "#000000");
    }

    #[test]
    fn default_config_has_site_title() {
        let config = SiteConfig::default();
        assert_eq!(config.site_title, "Gallery");
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
    fn default_full_index_is_off() {
        let config = SiteConfig::default();
        assert!(!config.full_index.generates);
        assert!(!config.full_index.show_link);
        assert_eq!(config.full_index.thumb_ratio, [4, 5]);
        assert_eq!(config.full_index.thumb_size, 400);
        assert_eq!(config.full_index.thumb_gap, "0.2rem");
    }

    #[test]
    fn default_thumbnail_gap_and_grid_padding() {
        let config = SiteConfig::default();
        assert_eq!(config.theme.thumbnail_gap, "0.2rem");
        assert_eq!(config.theme.grid_padding, "2rem");
    }

    #[test]
    fn default_assets_dir() {
        let config = SiteConfig::default();
        assert_eq!(config.assets_dir, "assets");
    }

    #[test]
    fn default_site_description_file() {
        let config = SiteConfig::default();
        assert_eq!(config.site_description_file, "site");
    }

    #[test]
    fn default_processing_config() {
        let config = SiteConfig::default();
        assert_eq!(config.processing.max_processes, None);
    }

    // ----- sparse layer parsing through load_config -----

    #[test]
    fn parse_custom_site_title() {
        let tmp = TempDir::new().unwrap();
        write_config(tmp.path(), r#"site_title = "My Portfolio""#);
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.site_title, "My Portfolio");
    }

    #[test]
    fn parse_partial_colors_only() {
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r##"
[colors.light]
background = "#fafafa"
"##,
        );
        let config = load_config(tmp.path()).unwrap();
        // Overridden value
        assert_eq!(config.colors.light.background, "#fafafa");
        // Sibling defaults preserved
        assert_eq!(config.colors.light.text, "#111111");
        assert_eq!(config.colors.dark.background, "#000000");
        // Unrelated section defaults preserved
        assert_eq!(config.images.sizes, vec![800, 1400, 2080]);
    }

    #[test]
    fn parse_image_settings() {
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r##"
[thumbnails]
aspect_ratio = [1, 1]

[images]
sizes = [400, 800]
quality = 85
"##,
        );
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.thumbnails.aspect_ratio, [1, 1]);
        assert_eq!(config.images.sizes, vec![400, 800]);
        assert_eq!(config.images.quality, 85);
        assert_eq!(config.colors.light.background, "#ffffff");
    }

    #[test]
    fn parse_full_index_settings() {
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r##"
[full_index]
generates = true
show_link = true
thumb_ratio = [4, 4]
thumb_size = 1000
thumb_gap = "0.5rem"
"##,
        );
        let config = load_config(tmp.path()).unwrap();
        assert!(config.full_index.generates);
        assert!(config.full_index.show_link);
        assert_eq!(config.full_index.thumb_ratio, [4, 4]);
        assert_eq!(config.full_index.thumb_size, 1000);
        assert_eq!(config.full_index.thumb_gap, "0.5rem");
    }

    #[test]
    fn full_index_partial_preserves_defaults() {
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r##"
[full_index]
generates = true
"##,
        );
        let config = load_config(tmp.path()).unwrap();
        assert!(config.full_index.generates);
        assert!(!config.full_index.show_link);
        assert_eq!(config.full_index.thumb_ratio, [4, 5]);
        assert_eq!(config.full_index.thumb_size, 400);
    }

    #[test]
    fn parse_partial_theme_mat_x_only() {
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r#"
[theme.mat_x]
size = "5vw"
"#,
        );
        let config = load_config(tmp.path()).unwrap();
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
        assert_eq!(config.theme.thumbnail_gap, "0.2rem");
        assert_eq!(config.theme.grid_padding, "2rem");
    }

    #[test]
    fn parse_partial_colors_light_keeps_dark_defaults() {
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r##"
[colors.light]
background = "#fafafa"
text = "#222222"
"##,
        );
        let config = load_config(tmp.path()).unwrap();
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
    fn parse_partial_font_weight_only() {
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r#"
[font]
weight = "300"
"#,
        );
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.font.weight, "300");
        assert_eq!(config.font.font, "Noto Sans");
        assert_eq!(config.font.font_type, FontType::Sans);
    }

    #[test]
    fn parse_thumbnail_gap_and_grid_padding() {
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r#"
[theme]
thumbnail_gap = "0.5rem"
grid_padding = "1rem"
"#,
        );
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.theme.thumbnail_gap, "0.5rem");
        assert_eq!(config.theme.grid_padding, "1rem");
    }

    #[test]
    fn parse_processing_config() {
        let tmp = TempDir::new().unwrap();
        write_config(tmp.path(), "[processing]\nmax_processes = 4\n");
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.processing.max_processes, Some(4));
    }

    #[test]
    fn parse_config_without_processing_uses_default() {
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r##"
[colors.light]
background = "#fafafa"
"##,
        );
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.processing.max_processes, None);
    }

    #[test]
    fn parse_custom_assets_dir() {
        let tmp = TempDir::new().unwrap();
        write_config(tmp.path(), r#"assets_dir = "site-assets""#);
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.assets_dir, "site-assets");
    }

    #[test]
    fn parse_custom_site_description_file() {
        let tmp = TempDir::new().unwrap();
        write_config(tmp.path(), r#"site_description_file = "intro""#);
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.site_description_file, "intro");
    }

    #[test]
    fn parse_multiple_sections_independently() {
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r##"
[colors.dark]
background = "#1a1a1a"

[font]
font = "Lora"
font_type = "serif"
"##,
        );
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.colors.dark.background, "#1a1a1a");
        assert_eq!(config.colors.dark.text, "#fafafa");
        assert_eq!(config.colors.light.background, "#ffffff");
        assert_eq!(config.font.font, "Lora");
        assert_eq!(config.font.font_type, FontType::Serif);
        assert_eq!(config.font.weight, "600");
        assert_eq!(config.images.quality, 90);
        assert_eq!(config.thumbnails.aspect_ratio, [4, 5]);
        assert_eq!(config.theme.mat_x.size, "3vw");
    }

    // ----- error rendering -----

    #[test]
    fn toml_error_carries_path_and_source_text() {
        let tmp = TempDir::new().unwrap();
        // Unquoted CSS value — the same class of mistake that produced the
        // original "expected newline, `#`" error we want rich rendering for.
        write_config(tmp.path(), "[theme]\nthumbnail_gap = 0.2rem\n");
        let err = load_config(tmp.path()).unwrap_err();
        match &err {
            ConfigError::Toml {
                path,
                source_text,
                source,
            } => {
                assert!(path.ends_with("config.toml"));
                assert!(source_text.contains("thumbnail_gap"));
                assert!(source.span().is_some());
            }
            other => panic!("expected Toml variant, got {:?}", other),
        }
    }

    #[test]
    fn to_clapfig_error_wraps_parse_failure() {
        let tmp = TempDir::new().unwrap();
        write_config(tmp.path(), "[theme]\nthumbnail_gap = 0.2rem\n");
        let err = load_config(tmp.path()).unwrap_err();
        let clap_err = err
            .to_clapfig_error()
            .expect("parse errors should be convertible to ClapfigError");
        let (path, parse_err, source_text) = clap_err
            .parse_error()
            .expect("ClapfigError should be a ParseError");
        assert!(path.ends_with("config.toml"));
        assert!(parse_err.span().is_some());
        assert!(source_text.unwrap().contains("thumbnail_gap"));
    }

    #[test]
    fn to_clapfig_error_is_none_for_validation_failure() {
        let err = ConfigError::Validation("quality out of range".into());
        assert!(err.to_clapfig_error().is_none());
    }

    #[test]
    fn clapfig_render_plain_includes_path_and_snippet() {
        let tmp = TempDir::new().unwrap();
        write_config(tmp.path(), "[theme]\nthumbnail_gap = 0.2rem\n");
        let err = load_config(tmp.path()).unwrap_err();
        let clap_err = err.to_clapfig_error().unwrap();
        let out = clapfig::render::render_plain(&clap_err);
        assert!(out.contains("config.toml"), "missing path in {out}");
        assert!(
            out.contains("thumbnail_gap"),
            "missing source snippet in {out}"
        );
        assert!(out.contains('^'), "missing caret in {out}");
    }

    // ----- load_config behavior -----

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
        write_config(
            tmp.path(),
            r##"
[colors.light]
background = "#123456"
text = "#abcdef"
"##,
        );
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.colors.light.background, "#123456");
        assert_eq!(config.colors.light.text, "#abcdef");
        assert_eq!(config.colors.dark.background, "#000000");
    }

    #[test]
    fn load_config_invalid_toml_is_error() {
        let tmp = TempDir::new().unwrap();
        write_config(tmp.path(), "this is not valid toml [[[");
        let result = load_config(tmp.path());
        assert!(matches!(result, Err(ConfigError::Toml { .. })));
    }

    #[test]
    fn load_config_unknown_keys_is_error() {
        let tmp = TempDir::new().unwrap();
        write_config(tmp.path(), "unknown_key = \"foo\"\n");
        let result = load_config(tmp.path());
        assert!(result.is_err());
    }

    #[test]
    fn load_config_validates_values() {
        let tmp = TempDir::new().unwrap();
        write_config(tmp.path(), "[images]\nquality = 200\n");
        let result = load_config(tmp.path());
        assert!(matches!(result, Err(ConfigError::Validation(_))));
    }

    // ----- validate() unit checks -----

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
    fn full_index_validation_rejects_zero_ratio() {
        let mut config = SiteConfig::default();
        config.full_index.thumb_ratio = [0, 1];
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

    // ----- effective_threads -----

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

    // ----- CSS generation -----

    #[test]
    fn generate_css_uses_config_colors() {
        let mut config = SiteConfig::default();
        config.colors.light.background = "#f0f0f0".to_string();
        config.colors.dark.background = "#1a1a1a".to_string();
        let css = generate_color_css(&config.colors);
        assert!(css.contains("--color-bg: #f0f0f0"));
        assert!(css.contains("--color-bg: #1a1a1a"));
    }

    #[test]
    fn generate_css_includes_all_variables() {
        let config = SiteConfig::default();
        let css = generate_color_css(&config.colors);
        assert!(css.contains("--color-bg:"));
        assert!(css.contains("--color-text:"));
        assert!(css.contains("--color-text-muted:"));
        assert!(css.contains("--color-border:"));
        assert!(css.contains("--color-link:"));
        assert!(css.contains("--color-link-hover:"));
    }

    #[test]
    fn generate_css_includes_dark_mode_media_query() {
        let config = SiteConfig::default();
        let css = generate_color_css(&config.colors);
        assert!(css.contains("@media (prefers-color-scheme: dark)"));
    }

    #[test]
    fn mat_x_to_css() {
        let config = SiteConfig::default();
        assert_eq!(config.theme.mat_x.to_css(), "clamp(1rem, 3vw, 2.5rem)");
    }

    #[test]
    fn generate_theme_css_includes_mat_variables() {
        let config = SiteConfig::default();
        let css = generate_theme_css(&config.theme);
        assert!(css.contains("--mat-x: clamp(1rem, 3vw, 2.5rem)"));
        assert!(css.contains("--mat-y: clamp(2rem, 6vw, 5rem)"));
        assert!(css.contains("--thumbnail-gap: 0.2rem"));
        assert!(css.contains("--grid-padding: 2rem"));
    }

    // ----- font helpers -----

    #[test]
    fn default_font_is_google() {
        let config = SiteConfig::default();
        assert!(!config.font.is_local());
        assert!(config.font.stylesheet_url().is_some());
        assert!(config.font.font_face_css().is_none());
    }

    #[test]
    fn local_font_has_no_stylesheet_url() {
        let mut config = SiteConfig::default();
        config.font.source = Some("fonts/MyFont.woff2".to_string());
        assert!(config.font.is_local());
        assert!(config.font.stylesheet_url().is_none());
    }

    #[test]
    fn local_font_generates_font_face_css() {
        let mut config = SiteConfig::default();
        config.font.font = "My Custom Font".to_string();
        config.font.weight = "400".to_string();
        config.font.font_type = FontType::Sans;
        config.font.source = Some("fonts/MyFont.woff2".to_string());
        let css = config.font.font_face_css().unwrap();
        assert!(css.contains("@font-face"));
        assert!(css.contains(r#"font-family: "My Custom Font""#));
        assert!(css.contains(r#"url("/fonts/MyFont.woff2")"#));
        assert!(css.contains(r#"format("woff2")"#));
        assert!(css.contains("font-weight: 400"));
        assert!(css.contains("font-display: swap"));
    }

    #[test]
    fn parse_font_with_source() {
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r#"
[font]
font = "My Font"
weight = "400"
source = "fonts/myfont.woff2"
"#,
        );
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.font.font, "My Font");
        assert_eq!(config.font.source.as_deref(), Some("fonts/myfont.woff2"));
        assert!(config.font.is_local());
    }

    #[test]
    fn parse_font_source_preserves_other_fields() {
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r#"
[font]
source = "fonts/custom.woff2"
"#,
        );
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.font.font, "Noto Sans");
        assert_eq!(config.font.weight, "600");
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
        let mut config = SiteConfig::default();
        config.font.font = "Local Font".to_string();
        config.font.weight = "700".to_string();
        config.font.font_type = FontType::Serif;
        config.font.source = Some("fonts/local.woff2".to_string());
        let css = generate_font_css(&config.font);
        assert!(css.contains("@font-face"));
        assert!(css.contains("--font-family:"));
        assert!(css.contains("--font-weight: 700"));
    }

    #[test]
    fn generate_font_css_no_font_face_for_google() {
        let config = SiteConfig::default();
        let css = generate_font_css(&config.font);
        assert!(!css.contains("@font-face"));
        assert!(css.contains("--font-family:"));
    }

    // ----- unknown key rejection -----

    #[test]
    fn unknown_key_rejected_via_load_config() {
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r#"
[images]
qualty = 90
"#,
        );
        let result = load_config(tmp.path());
        assert!(result.is_err());
    }

    // ----- auto-indexing -----

    #[test]
    fn default_auto_indexing_is_off() {
        let config = SiteConfig::default();
        assert!(!config.auto_indexing.sync_source_files);
        assert_eq!(config.auto_indexing.spacing, 1);
        assert_eq!(config.auto_indexing.padding, 3);
    }

    #[test]
    fn parse_auto_indexing_full() {
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r#"
[auto_indexing]
sync_source_files = true
spacing = 2
padding = 4
"#,
        );
        let config = load_config(tmp.path()).unwrap();
        assert!(config.auto_indexing.sync_source_files);
        assert_eq!(config.auto_indexing.spacing, 2);
        assert_eq!(config.auto_indexing.padding, 4);
    }

    #[test]
    fn parse_auto_indexing_partial_preserves_defaults() {
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r#"
[auto_indexing]
sync_source_files = true
"#,
        );
        let config = load_config(tmp.path()).unwrap();
        assert!(config.auto_indexing.sync_source_files);
        assert_eq!(config.auto_indexing.spacing, 1);
        assert_eq!(config.auto_indexing.padding, 3);
    }

    #[test]
    fn auto_indexing_old_auto_field_rejected() {
        // Phase 5 migration: the old enum-based `auto` field is gone;
        // confique's deny_unknown_fields rejects configs still using it
        // so users get a loud error pointing at `sync_source_files`
        // rather than silent no-ops. We assert on the specific error
        // shape so a general "config broken" change couldn't accidentally
        // let this regress.
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r#"
[auto_indexing]
auto = "source_only"
"#,
        );
        let err_text = load_config(tmp.path()).unwrap_err().to_string();
        assert!(
            err_text.contains("unknown field") && err_text.contains("`auto`"),
            "expected unknown-field error naming `auto`, got: {err_text}"
        );
        assert!(
            err_text.contains("sync_source_files"),
            "error should point users at the new field `sync_source_files`, got: {err_text}"
        );
    }

    #[test]
    fn auto_indexing_unknown_key_rejected() {
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r#"
[auto_indexing]
spaceing = 2
"#,
        );
        assert!(load_config(tmp.path()).is_err());
    }

    #[test]
    fn auto_indexing_partial_preserves_other_sections() {
        let tmp = TempDir::new().unwrap();
        write_config(
            tmp.path(),
            r#"
[auto_indexing]
spacing = 0
"#,
        );
        let config = load_config(tmp.path()).unwrap();
        assert_eq!(config.auto_indexing.spacing, 0);
        assert!(!config.auto_indexing.sync_source_files);
        assert_eq!(config.auto_indexing.padding, 3);
        assert_eq!(config.images.sizes, vec![800, 1400, 2080]);
        assert_eq!(config.colors.light.background, "#ffffff");
    }

    #[test]
    fn auto_indexing_spacing_upper_bound_accepted() {
        let mut config = SiteConfig::default();
        config.auto_indexing.spacing = 9;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn auto_indexing_spacing_out_of_range_rejected() {
        let mut config = SiteConfig::default();
        config.auto_indexing.spacing = 10;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("spacing"));
    }

    #[test]
    fn auto_indexing_padding_upper_bound_accepted() {
        let mut config = SiteConfig::default();
        config.auto_indexing.padding = 12;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn auto_indexing_padding_out_of_range_rejected() {
        let mut config = SiteConfig::default();
        config.auto_indexing.padding = 13;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("padding"));
    }
}
