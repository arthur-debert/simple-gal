//! HTML site generation.
//!
//! Stage 3 of the Simple Gal build pipeline. Takes the processed manifest and
//! generates the final static HTML site.
//!
//! ## Generated Pages
//!
//! - **Index page** (`/index.html`): Gallery list showing top-level album/group cards
//! - **Gallery-list pages** (`/{group}/index.html`): Gallery list for a container directory, showing cards for each child album or sub-group
//! - **Album pages** (`/{album}/index.html`): Thumbnail grid for an album
//! - **Image pages** (`/{album}/{n}-{slug}.html`): Full-screen image viewer with navigation
//! - **Content pages** (`/{slug}.html`): Markdown pages (e.g. about, contact)
//!
//! ## Features
//!
//! - **Responsive images**: Uses AVIF srcset for responsive images
//! - **Collapsible navigation**: Details/summary for mobile-friendly nav
//! - **Keyboard navigation**: Arrow keys and swipe gestures for image browsing
//! - **View transitions**: Smooth page-to-page animations (where supported)
//! - **Configurable colors**: CSS custom properties generated from config.toml
//!
//! ## Output Structure
//!
//! ```text
//! dist/
//! ├── index.html                 # Gallery list (top-level cards)
//! ├── about.html                 # Content page (from 040-about.md)
//! ├── Landscapes/
//! │   ├── index.html             # Album page (thumbnail grid)
//! │   ├── 1-dawn.html            # Image viewer pages
//! │   ├── 2-sunset.html
//! │   ├── 001-dawn-800.avif      # Processed images (copied)
//! │   └── ...
//! └── Travel/
//!     ├── index.html             # Gallery-list page (child album cards)
//!     ├── Japan/
//!     │   ├── index.html         # Album page
//!     │   └── ...
//!     └── Italy/
//!         └── ...
//! ```
//!
//! ## CSS and JavaScript
//!
//! Static assets are embedded at compile time:
//! - `static/style.css`: Base styles (colors injected from config)
//! - `static/nav.js`: Keyboard and touch navigation
//!
//! ## Custom Snippets
//!
//! Users can inject custom content by placing convention files in `assets/`:
//! - `custom.css`: Linked after the main `<style>` block for CSS overrides
//! - `head.html`: Raw HTML injected at the end of `<head>` (analytics, meta tags)
//! - `body-end.html`: Raw HTML injected before `</body>` (tracking scripts, widgets)
//!
//! ## HTML Generation
//!
//! Uses [maud](https://maud.lambda.xyz/) for compile-time HTML templating.
//! Templates are type-safe Rust code with automatic XSS escaping.

use crate::config::{self, SiteConfig};
use crate::types::{NavItem, Page};
use maud::{DOCTYPE, Markup, PreEscaped, html};
use pulldown_cmark::{Parser, html as md_html};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum GenerateError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Processed manifest from stage 2
#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub navigation: Vec<NavItem>,
    pub albums: Vec<Album>,
    #[serde(default)]
    pub pages: Vec<Page>,
    #[serde(default)]
    pub description: Option<String>,
    pub config: SiteConfig,
}

#[derive(Debug, Deserialize)]
pub struct Album {
    pub path: String,
    pub title: String,
    pub description: Option<String>,
    pub thumbnail: String,
    pub images: Vec<Image>,
    pub in_nav: bool,
    /// Resolved config for this album (available for future per-album theming).
    #[allow(dead_code)]
    pub config: SiteConfig,
    #[serde(default)]
    #[allow(dead_code)]
    pub support_files: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Image {
    pub number: u32,
    #[allow(dead_code)]
    pub source_path: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub dimensions: (u32, u32),
    pub generated: BTreeMap<String, GeneratedVariant>,
    pub thumbnail: String,
    #[serde(default)]
    pub full_index_thumbnail: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GeneratedVariant {
    pub avif: String,
    #[allow(dead_code)]
    pub width: u32,
    #[allow(dead_code)]
    pub height: u32,
}

const CSS_STATIC: &str = include_str!("../static/style.css");
const JS: &str = include_str!("../static/nav.js");
const SW_JS_TEMPLATE: &str = include_str!("../static/sw.js");
// We embed default icons so every installation is a valid PWA out of the box.
// Users can override these by placing files in their assets/ directory.
const ICON_192: &[u8] = include_bytes!("../static/icon-192.png");
const ICON_512: &[u8] = include_bytes!("../static/icon-512.png");
const APPLE_TOUCH_ICON: &[u8] = include_bytes!("../static/apple-touch-icon.png");
const FAVICON_PNG: &[u8] = include_bytes!("../static/favicon.png");

/// Compute the `sizes` attribute for a responsive image based on its aspect ratio
/// and the maximum generated width. The image frame CSS constrains display to
/// `min(container-width, container-height * aspect-ratio)`, so for portrait images
/// the height constraint dominates and the displayed width is much less than 100vw.
fn image_sizes_attr(aspect_ratio: f64, max_generated_width: u32) -> String {
    // ~90vh accounts for header + mat; multiply by aspect ratio for the
    // height-constrained case (portrait images on wide screens).
    let vh_factor = 90.0 * aspect_ratio;
    // Cap so the browser never requests more than our largest variant.
    let cap = format!("{}px", max_generated_width);
    if vh_factor >= 100.0 {
        // Wide landscape: width-constrained, ~100vw on mobile, ~95vw on desktop
        format!("(max-width: 800px) min(100vw, {cap}), min(95vw, {cap})")
    } else {
        // Portrait / square: height-constrained on desktop
        format!("(max-width: 800px) min(100vw, {cap}), min({vh_factor:.1}vh, {cap})")
    }
}

/// An entry in a gallery-list page (index or container page).
struct GalleryEntry {
    title: String,
    path: String,
    thumbnail: Option<String>,
}

/// Find a thumbnail for a nav item by walking into its first child recursively.
fn find_nav_thumbnail(item: &NavItem, albums: &[Album]) -> Option<String> {
    if item.children.is_empty() {
        // Leaf: find the matching album
        albums
            .iter()
            .find(|a| a.path == item.path)
            .map(|a| a.thumbnail.clone())
    } else {
        // Container: recurse into first child
        item.children
            .first()
            .and_then(|c| find_nav_thumbnail(c, albums))
    }
}

/// Build gallery entries from nav children for a gallery-list page.
fn collect_gallery_entries(children: &[NavItem], albums: &[Album]) -> Vec<GalleryEntry> {
    children
        .iter()
        .map(|item| GalleryEntry {
            title: item.title.clone(),
            path: item.path.clone(),
            thumbnail: find_nav_thumbnail(item, albums),
        })
        .collect()
}

/// Walk the navigation tree and find breadcrumb segments for a given path.
///
/// Returns a list of (title, path) pairs from root to the matching node (exclusive).
fn path_to_breadcrumb_segments<'a>(
    path: &str,
    navigation: &'a [NavItem],
) -> Vec<(&'a str, &'a str)> {
    fn find_segments<'a>(
        path: &str,
        items: &'a [NavItem],
        segments: &mut Vec<(&'a str, &'a str)>,
    ) -> bool {
        for item in items {
            if item.path == path {
                return true;
            }
            if path.starts_with(&format!("{}/", item.path)) {
                segments.push((&item.title, &item.path));
                if find_segments(path, &item.children, segments) {
                    return true;
                }
                segments.pop();
            }
        }
        false
    }

    let mut segments = Vec::new();
    find_segments(path, navigation, &mut segments);
    segments
}

/// User-provided snippets discovered via convention files in the assets directory.
///
/// Drop any of these files into your `assets/` directory to inject custom content:
/// - `custom.css` → `<link rel="stylesheet">` after the main `<style>` block
/// - `head.html` → raw HTML at the end of `<head>`
/// - `body-end.html` → raw HTML before `</body>`
#[derive(Debug, Default)]
struct CustomSnippets {
    /// Whether `custom.css` exists in the output directory.
    has_custom_css: bool,
    /// Raw HTML to inject at the end of `<head>`.
    head_html: Option<String>,
    /// Raw HTML to inject before `</body>`.
    body_end_html: Option<String>,
}

/// Detect convention-based custom snippet files in the output directory.
///
/// Called after assets are copied so user files are already in place.
fn detect_custom_snippets(output_dir: &Path) -> CustomSnippets {
    CustomSnippets {
        has_custom_css: output_dir.join("custom.css").exists(),
        head_html: fs::read_to_string(output_dir.join("head.html")).ok(),
        body_end_html: fs::read_to_string(output_dir.join("body-end.html")).ok(),
    }
}

/// Zero-padding width for image indices, based on album size.
pub(crate) fn index_width(total: usize) -> usize {
    match total {
        0..=9 => 1,
        10..=99 => 2,
        100..=999 => 3,
        _ => 4,
    }
}

/// Build an image page directory name like `"02-My-Title/"` or `"02/"` (when no title).
///
/// The directory name mirrors the display label shown in the header/breadcrumb
/// (`"02. My Title"`) but URL-escaped: dots and spaces become hyphens, consecutive
/// hyphens are collapsed.
///
/// Image pages are directories with an `index.html` inside, so that static
/// servers can serve them without requiring `.html` in the URL.
pub(crate) fn image_page_url(position: usize, total: usize, title: Option<&str>) -> String {
    let width = index_width(total);
    match title {
        Some(t) => {
            let escaped = escape_for_url(t);
            format!("{:0>width$}-{}/", position, escaped)
        }
        None => format!("{:0>width$}/", position),
    }
}

/// Escape a display title for use in URL paths.
///
/// Lowercases, replaces spaces/dots/underscores with hyphens, and collapses consecutive hyphens.
fn escape_for_url(title: &str) -> String {
    let mut result = String::with_capacity(title.len());
    let mut prev_dash = false;
    for c in title.chars() {
        if c == ' ' || c == '.' || c == '_' {
            if !prev_dash {
                result.push('-');
            }
            prev_dash = true;
        } else {
            result.extend(c.to_lowercase());
            prev_dash = false;
        }
    }
    result.trim_matches('-').to_string()
}

const SHORT_CAPTION_MAX_LEN: usize = 160;

/// Whether a description is short enough to display as an inline caption.
///
/// Short captions (≤160 chars, single line) are rendered as centered text
/// directly beneath the image. Longer or multi-line descriptions get a
/// scrollable container instead.
fn is_short_caption(text: &str) -> bool {
    text.len() <= SHORT_CAPTION_MAX_LEN && !text.contains('\n')
}

// ============================================================================
// Open Graph metadata
// ============================================================================

/// Target width for og:image (Facebook/Twitter recommend 1200px wide, 1.91:1).
/// Chat-app scrapers (WhatsApp, iMessage, Slack) prefer images under ~300KB;
/// a 1200-wide AVIF variant comfortably fits.
const OG_IMAGE_TARGET_WIDTH: u32 = 1200;

/// Breadcrumb separator used in og:description. Matches the on-page crumb
/// (`site_header`) so the preview text echoes the site's visible hierarchy.
const OG_CRUMB_SEP: &str = " › ";

/// Open Graph / Twitter Card metadata for a single page.
///
/// Populated only when `site.base_url` is set in config — that's a hard
/// requirement because og:image and og:url MUST be absolute URLs for
/// scrapers like WhatsApp and iMessage to resolve them. When `base_url`
/// is unset, pages render without any OG tags.
struct OgMeta {
    /// `og:title` — typically the page's short label (album title, image
    /// label, or site title for the index).
    title: String,
    /// `og:description` — the breadcrumb chain (e.g. "Gallery › NY › Night")
    /// so previews are meaningful even when the album/image has no prose
    /// description, which is common for photography.
    description: String,
    /// `og:image` — absolute URL to a ~1200px-wide variant.
    image_url: String,
    /// `og:url` — absolute URL of this page (canonical).
    page_url: String,
    /// `og:site_name` — always the site title.
    site_name: String,
}

/// Pick the best image variant for og:image: the smallest variant whose
/// width is ≥ OG_IMAGE_TARGET_WIDTH, falling back to the largest available
/// if every variant is smaller than the target.
fn pick_og_variant(image: &Image) -> Option<&GeneratedVariant> {
    let variants: Vec<&GeneratedVariant> = image.generated.values().collect();
    variants
        .iter()
        .filter(|v| v.width >= OG_IMAGE_TARGET_WIDTH)
        .min_by_key(|v| v.width)
        .or_else(|| variants.iter().max_by_key(|v| v.width))
        .copied()
}

/// Walk the navigation tree and return the first Album reachable from a
/// nav item (recursing into containers). Used to find a cover image for
/// gallery-list pages (index and container pages), which have no image
/// of their own.
fn first_album_in_nav<'a>(items: &[NavItem], albums: &'a [Album]) -> Option<&'a Album> {
    for item in items {
        if item.children.is_empty() {
            if let Some(a) = albums.iter().find(|a| a.path == item.path) {
                return Some(a);
            }
        } else if let Some(a) = first_album_in_nav(&item.children, albums) {
            return Some(a);
        }
    }
    None
}

/// Join a base URL and a root-relative path into an absolute URL, tolerating
/// trailing slashes on the base and leading slashes on the path.
fn absolute_url(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        format!("{}/", base)
    } else {
        format!("{}/{}", base, path)
    }
}

/// Build an og:description by joining site title + breadcrumb segments + any
/// trailing labels (album title, image label) with `OG_CRUMB_SEP`.
///
/// Photography sites often have untitled images or captionless albums, so a
/// generic "Photo from the gallery" would be unhelpful. Echoing the crumb
/// ("Gallery › NY › Night › 03. City") tells the reader exactly what they're
/// about to open.
fn og_description(site_title: &str, segments: &[(&str, &str)], trailing: &[&str]) -> String {
    let mut parts: Vec<&str> = Vec::with_capacity(1 + segments.len() + trailing.len());
    parts.push(site_title);
    for (seg_title, _) in segments {
        parts.push(seg_title);
    }
    parts.extend(trailing.iter().copied());
    parts.join(OG_CRUMB_SEP)
}

/// Find the image that corresponds to an album's displayed cover thumbnail
/// (set from `preview_image` in the process stage: may be the first image, a
/// user-configured one, or a `NNN-thumb`-designated image). Matching by
/// `thumbnail` path keeps the OG image in sync with the card thumbnail the
/// user actually sees in the gallery grid; `.first()` is a defensive fallback
/// in case the paths ever disagree.
fn album_cover_image(album: &Album) -> Option<&Image> {
    album
        .images
        .iter()
        .find(|image| image.thumbnail == album.thumbnail)
        .or_else(|| album.images.first())
}

/// Build OgMeta for an album page. Returns None when the album has no
/// generated image variants (empty album) — without a cover image there's
/// no meaningful preview to emit.
fn build_og_for_album(
    base_url: &str,
    album: &Album,
    navigation: &[NavItem],
    site_title: &str,
) -> Option<OgMeta> {
    let cover = album_cover_image(album)?;
    let variant = pick_og_variant(cover)?;
    let segments = path_to_breadcrumb_segments(&album.path, navigation);
    Some(OgMeta {
        title: album.title.clone(),
        description: og_description(site_title, &segments, &[&album.title]),
        image_url: absolute_url(base_url, &variant.avif),
        page_url: absolute_url(base_url, &format!("{}/", album.path)),
        site_name: site_title.to_string(),
    })
}

/// Build OgMeta for an image page. Returns None when the image has no
/// generated variants (shouldn't happen in practice but guarded for safety).
fn build_og_for_image(
    base_url: &str,
    album: &Album,
    image: &Image,
    image_idx: usize,
    navigation: &[NavItem],
    site_title: &str,
) -> Option<OgMeta> {
    let variant = pick_og_variant(image)?;
    let total = album.images.len();
    let image_label = format_image_label(image_idx + 1, total, image.title.as_deref());
    let segments = path_to_breadcrumb_segments(&album.path, navigation);
    let page_url_path = format!(
        "{}/{}",
        album.path,
        image_page_url(image_idx + 1, total, image.title.as_deref())
    );
    Some(OgMeta {
        title: image
            .title
            .clone()
            .unwrap_or_else(|| format!("{} — {}", album.title, image_label)),
        description: og_description(site_title, &segments, &[&album.title, &image_label]),
        image_url: absolute_url(base_url, &variant.avif),
        page_url: absolute_url(base_url, &page_url_path),
        site_name: site_title.to_string(),
    })
}

/// Build OgMeta for a gallery-list page (index root or a container like /NY/).
///
/// `path` is empty for the root index or e.g. `"NY"` for a container. `title`
/// is the page's display title (site_title for root, container's title
/// otherwise). Uses the first leaf album reachable from `nav_scope` as the
/// cover image source. Returns None if no cover image is findable.
fn build_og_for_gallery_list(
    base_url: &str,
    title: &str,
    path: &str,
    nav_scope: &[NavItem],
    navigation: &[NavItem],
    albums: &[Album],
    site_title: &str,
) -> Option<OgMeta> {
    let cover_album = first_album_in_nav(nav_scope, albums)?;
    let cover_image = album_cover_image(cover_album)?;
    let variant = pick_og_variant(cover_image)?;
    let page_url = if path.is_empty() {
        absolute_url(base_url, "")
    } else {
        absolute_url(base_url, &format!("{}/", path))
    };
    let description = if path.is_empty() {
        site_title.to_string()
    } else {
        let segments = path_to_breadcrumb_segments(path, navigation);
        og_description(site_title, &segments, &[title])
    };
    Some(OgMeta {
        title: title.to_string(),
        description,
        image_url: absolute_url(base_url, &variant.avif),
        page_url,
        site_name: site_title.to_string(),
    })
}

/// Render the `<meta>` tags for an OgMeta into a Markup fragment to be
/// inlined in `<head>`. Emits Open Graph + Twitter Card "summary_large_image"
/// (WhatsApp/iMessage/Slack all read OG; Twitter/X needs the twitter:card hint
/// to pick the big-image layout).
fn render_og_tags(og: &OgMeta) -> Markup {
    html! {
        meta property="og:type" content="website";
        meta property="og:site_name" content=(og.site_name);
        meta property="og:title" content=(og.title);
        meta property="og:description" content=(og.description);
        meta property="og:url" content=(og.page_url);
        meta property="og:image" content=(og.image_url);
        meta name="twitter:card" content="summary_large_image";
        meta name="twitter:title" content=(og.title);
        meta name="twitter:description" content=(og.description);
        meta name="twitter:image" content=(og.image_url);
    }
}

pub fn generate(
    manifest_path: &Path,
    processed_dir: &Path,
    output_dir: &Path,
    source_dir: &Path,
) -> Result<(), GenerateError> {
    let manifest_content = fs::read_to_string(manifest_path)?;
    let manifest: Manifest = serde_json::from_str(&manifest_content)?;

    // ── CSS assembly ──────────────────────────────────────────────────
    // The final CSS is built from THREE sources, injected in two places:
    //
    //   1. Google Font <link>  → emitted in <head> BEFORE <style>
    //      (see base_document() — font_url becomes a <link rel="stylesheet">)
    //      DO NOT use @import inside <style>; browsers ignore/delay it.
    //      For local fonts, this is skipped and @font-face is used instead.
    //
    //   2. Generated CSS vars  → config::generate_{color,theme,font}_css()
    //      Produces :root { --color-*, --mat-*, --font-*, … }
    //      For local fonts, also includes @font-face declaration.
    //      Prepended to the <style> block so vars are defined before use.
    //
    //   3. Static CSS rules    → static/style.css (compiled in via include_str!)
    //      References the vars above. MUST NOT redefine them — if a var
    //      needs to come from config, generate it in (2) and consume it here.
    //
    // When adding new config-driven CSS: generate the variable in config.rs,
    // wire it into this assembly, and reference it in static/style.css.
    // ────────────────────────────────────────────────────────────────────
    let font_url = manifest.config.font.stylesheet_url();
    let color_css = config::generate_color_css(&manifest.config.colors);
    let theme_css = config::generate_theme_css(&manifest.config.theme);
    let font_css = config::generate_font_css(&manifest.config.font);
    let css = format!(
        "{}\n\n{}\n\n{}\n\n{}",
        color_css, theme_css, font_css, CSS_STATIC
    );

    fs::create_dir_all(output_dir)?;

    // ── PWA assets ────────────────────────────────────────────────────
    // Written *before* copying user assets so the user can override any
    // of them by placing files in their assets/ directory.
    //
    // IMPORTANT: All PWA paths are absolute from the domain root
    // (/sw.js, /site.webmanifest, /icon-*.png, scope "/", start_url "/").
    // The generated site MUST be deployed at the root of its domain.
    // Subdirectory deployment (e.g. example.com/gallery/) is not supported
    // because the service worker scope, manifest paths, and cached asset
    // URLs would all need to be rewritten with the subpath prefix.
    // ────────────────────────────────────────────────────────────────────

    // 1. Dynamic Manifest (uses site title)
    let manifest_json = serde_json::json!({
        "name": manifest.config.site_title,
        "short_name": manifest.config.site_title,
        "icons": [
            {
                "src": "/icon-192.png",
                "sizes": "192x192",
                "type": "image/png"
            },
            {
                "src": "/icon-512.png",
                "sizes": "512x512",
                "type": "image/png"
            }
        ],
        "theme_color": "#ffffff",
        "background_color": "#ffffff",
        "display": "standalone",
        "scope": "/",
        "start_url": "/"
    });
    fs::write(
        output_dir.join("site.webmanifest"),
        serde_json::to_string_pretty(&manifest_json)?,
    )?;

    // 2. Dynamic Service Worker (uses package version for cache busting)
    // We replace the default cache name with one including the build version.
    let version = env!("CARGO_PKG_VERSION");
    let sw_content = SW_JS_TEMPLATE.replace(
        "const CACHE_NAME = 'simple-gal-v1';",
        &format!("const CACHE_NAME = 'simple-gal-v{}';", version),
    );
    fs::write(output_dir.join("sw.js"), sw_content)?;

    fs::write(output_dir.join("icon-192.png"), ICON_192)?;
    fs::write(output_dir.join("icon-512.png"), ICON_512)?;
    fs::write(output_dir.join("apple-touch-icon.png"), APPLE_TOUCH_ICON)?;
    fs::write(output_dir.join("favicon.png"), FAVICON_PNG)?;

    // Copy static assets (favicon, fonts, etc.) to output root
    let assets_path = source_dir.join(&manifest.config.assets_dir);
    if assets_path.is_dir() {
        copy_dir_recursive(&assets_path, output_dir)?;
    }

    // Copy processed images to output
    copy_dir_recursive(processed_dir, output_dir)?;

    // Detect favicon in output directory for <link rel="icon"> injection
    let favicon_href = detect_favicon(output_dir);

    // Detect convention-based custom snippets (custom.css, head.html, body-end.html)
    let snippets = detect_custom_snippets(output_dir);

    // Generate index page
    let index_og = manifest.config.base_url.as_deref().and_then(|base| {
        build_og_for_gallery_list(
            base,
            &manifest.config.site_title,
            "",
            &manifest.navigation,
            &manifest.navigation,
            &manifest.albums,
            &manifest.config.site_title,
        )
    });
    let index_html = render_index(
        &manifest,
        &css,
        font_url.as_deref(),
        favicon_href.as_deref(),
        &snippets,
        index_og.as_ref(),
    );
    fs::write(output_dir.join("index.html"), index_html.into_string())?;

    let show_all_photos = show_all_photos_link(&manifest.config);

    // Generate pages (content pages only, not link pages)
    for page in manifest.pages.iter().filter(|p| !p.is_link) {
        let page_html = render_page(
            page,
            &manifest.navigation,
            &manifest.pages,
            &css,
            font_url.as_deref(),
            &manifest.config.site_title,
            favicon_href.as_deref(),
            &snippets,
            show_all_photos,
        );
        let filename = format!("{}.html", page.slug);
        fs::write(output_dir.join(&filename), page_html.into_string())?;
    }

    // Generate gallery-list pages for container directories
    generate_gallery_list_pages(
        &manifest.navigation,
        &manifest.albums,
        &manifest.navigation,
        &manifest.pages,
        &css,
        font_url.as_deref(),
        &manifest.config.site_title,
        favicon_href.as_deref(),
        &snippets,
        show_all_photos,
        manifest.config.base_url.as_deref(),
        output_dir,
    )?;

    // Generate album pages
    for album in &manifest.albums {
        let album_dir = output_dir.join(&album.path);
        fs::create_dir_all(&album_dir)?;

        let album_og = manifest.config.base_url.as_deref().and_then(|base| {
            build_og_for_album(
                base,
                album,
                &manifest.navigation,
                &manifest.config.site_title,
            )
        });
        let album_html = render_album_page(
            album,
            &manifest.navigation,
            &manifest.pages,
            &css,
            font_url.as_deref(),
            &manifest.config.site_title,
            favicon_href.as_deref(),
            &snippets,
            show_all_photos,
            album_og.as_ref(),
        );
        fs::write(album_dir.join("index.html"), album_html.into_string())?;

        // Generate image pages
        for (idx, image) in album.images.iter().enumerate() {
            let prev = if idx > 0 {
                Some(&album.images[idx - 1])
            } else {
                None
            };
            let next = album.images.get(idx + 1);

            let image_og = manifest.config.base_url.as_deref().and_then(|base| {
                build_og_for_image(
                    base,
                    album,
                    image,
                    idx,
                    &manifest.navigation,
                    &manifest.config.site_title,
                )
            });
            let image_html = render_image_page(
                album,
                image,
                prev,
                next,
                &manifest.navigation,
                &manifest.pages,
                &css,
                font_url.as_deref(),
                &manifest.config.site_title,
                favicon_href.as_deref(),
                &snippets,
                show_all_photos,
                image_og.as_ref(),
            );
            let image_dir_name =
                image_page_url(idx + 1, album.images.len(), image.title.as_deref());
            let image_dir = album_dir.join(&image_dir_name);
            fs::create_dir_all(&image_dir)?;
            fs::write(image_dir.join("index.html"), image_html.into_string())?;
        }
    }

    // Site-wide "All Photos" page (opt-in via [full_index] generates = true)
    if manifest.config.full_index.generates {
        let all_photos_html = render_full_index_page(
            &manifest,
            &css,
            font_url.as_deref(),
            favicon_href.as_deref(),
            &snippets,
        );
        let all_photos_dir = output_dir.join("all-photos");
        fs::create_dir_all(&all_photos_dir)?;
        fs::write(
            all_photos_dir.join("index.html"),
            all_photos_html.into_string(),
        )?;
    }

    Ok(())
}

/// Check the output directory for common favicon files and return the href if found.
fn detect_favicon(output_dir: &Path) -> Option<String> {
    for (filename, _mime) in &[
        ("favicon.svg", "image/svg+xml"),
        ("favicon.ico", "image/x-icon"),
        ("favicon.png", "image/png"),
    ] {
        if output_dir.join(filename).exists() {
            return Some(format!("/{}", filename));
        }
    }
    None
}

/// Determine the MIME type for a favicon based on its extension.
fn favicon_type(href: &str) -> &'static str {
    if href.ends_with(".svg") {
        "image/svg+xml"
    } else if href.ends_with(".png") {
        "image/png"
    } else {
        "image/x-icon"
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if src_path.extension().map(|e| e != "json").unwrap_or(true) {
            // Skip manifest.json, copy everything else
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

// ============================================================================
// HTML Components
// ============================================================================

/// Renders the base HTML document structure.
///
/// Font loading: for Google Fonts, loaded via a `<link>` tag, NOT via
/// `@import` inside `<style>`. Browsers ignore or delay `@import` in
/// inline `<style>` blocks. For local fonts, `@font-face` is in the CSS
/// and `font_url` is `None`. See the CSS assembly comment in `generate()`.
#[allow(clippy::too_many_arguments)]
fn base_document(
    title: &str,
    css: &str,
    font_url: Option<&str>,
    body_class: Option<&str>,
    head_extra: Option<Markup>,
    favicon_href: Option<&str>,
    snippets: &CustomSnippets,
    og: Option<&OgMeta>,
    content: Markup,
) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { (title) }
                @if let Some(og) = og {
                    (render_og_tags(og))
                }
                // PWA links — absolute paths, requires root deployment (see PWA comment in generate())
                link rel="manifest" href="/site.webmanifest";
                link rel="apple-touch-icon" href="/apple-touch-icon.png";
                @if let Some(href) = favicon_href {
                    link rel="icon" type=(favicon_type(href)) href=(href);
                }
                // Google Font loaded as <link>, not @import — see generate().
                @if let Some(url) = font_url {
                    link rel="preconnect" href="https://fonts.googleapis.com";
                    link rel="preconnect" href="https://fonts.gstatic.com" crossorigin="";
                    link rel="stylesheet" href=(url);
                }
                style { (PreEscaped(css)) }
                // Custom CSS loaded after main styles so overrides win at equal specificity.
                @if snippets.has_custom_css {
                    link rel="stylesheet" href="/custom.css";
                }
                @if let Some(extra) = head_extra {
                    (extra)
                }
                script {
                    (PreEscaped(r#"
                        if ('serviceWorker' in navigator && location.protocol !== 'file:') {
                            window.addEventListener('load', () => {
                                navigator.serviceWorker.register('/sw.js');
                            });
                        }
                        window.addEventListener('beforeinstallprompt', e => e.preventDefault());
                    "#))
                }
                @if let Some(ref html) = snippets.head_html {
                    (PreEscaped(html))
                }
            }
            body class=[body_class] {
                (content)
                script { (PreEscaped(JS)) }
                @if let Some(ref html) = snippets.body_end_html {
                    (PreEscaped(html))
                }
            }
        }
    }
}

/// Renders the site header with breadcrumb and navigation
fn site_header(breadcrumb: Markup, nav: Markup) -> Markup {
    html! {
        header.site-header {
            nav.breadcrumb {
                (breadcrumb)
            }
            nav.site-nav {
                (nav)
            }
        }
    }
}

/// Renders the navigation menu (hamburger style, slides from right).
///
/// Albums are listed first, then a separator, then pages (numbered pages only).
/// Link pages render as direct external links; content pages link to `/{slug}.html`.
///
/// When `show_all_photos` is true, an "All Photos" item is appended after the
/// album list — it points at `/all-photos/` which is rendered only when
/// `[full_index] generates = true`.
pub fn render_nav(
    items: &[NavItem],
    current_path: &str,
    pages: &[Page],
    show_all_photos: bool,
) -> Markup {
    let nav_pages: Vec<&Page> = pages.iter().filter(|p| p.in_nav).collect();
    let all_photos_current = current_path == "all-photos";

    html! {
        input.nav-toggle type="checkbox" id="nav-toggle";
        label.nav-hamburger for="nav-toggle" {
            span.hamburger-line {}
            span.hamburger-line {}
            span.hamburger-line {}
        }
        div.nav-panel {
            label.nav-close for="nav-toggle" { "×" }
            ul {
                @for item in items {
                    (render_nav_item(item, current_path))
                }
                @if show_all_photos {
                    li class=[all_photos_current.then_some("current")] {
                        a href="/all-photos/" { "All Photos" }
                    }
                }
                @if !nav_pages.is_empty() {
                    li.nav-separator role="separator" {}
                    @for page in &nav_pages {
                        @if page.is_link {
                            li {
                                a href=(page.body.trim()) target="_blank" rel="noopener" {
                                    (page.link_title)
                                }
                            }
                        } @else {
                            @let is_current = current_path == page.slug;
                            li class=[is_current.then_some("current")] {
                                a href={ "/" (page.slug) ".html" } { (page.link_title) }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Renders a single navigation item (may have children)
fn render_nav_item(item: &NavItem, current_path: &str) -> Markup {
    let is_current =
        item.path == current_path || current_path.starts_with(&format!("{}/", item.path));

    html! {
        li class=[is_current.then_some("current")] {
            @if item.children.is_empty() {
                a href={ "/" (item.path) "/" } { (item.title) }
            } @else {
                a.nav-group href={ "/" (item.path) "/" } { (item.title) }
                ul {
                    @for child in &item.children {
                        (render_nav_item(child, current_path))
                    }
                }
            }
        }
    }
}

// ============================================================================
// Page Renderers
// ============================================================================

/// Renders the index/home page with album grid.
///
/// Delegates to `render_gallery_list_page` — the index is just a gallery-list
/// of top-level navigation entries.
fn render_index(
    manifest: &Manifest,
    css: &str,
    font_url: Option<&str>,
    favicon_href: Option<&str>,
    snippets: &CustomSnippets,
    og: Option<&OgMeta>,
) -> Markup {
    render_gallery_list_page(
        &manifest.config.site_title,
        "",
        &collect_gallery_entries(&manifest.navigation, &manifest.albums),
        manifest.description.as_deref(),
        &manifest.navigation,
        &manifest.pages,
        css,
        font_url,
        &manifest.config.site_title,
        favicon_href,
        snippets,
        show_all_photos_link(&manifest.config),
        og,
    )
}

/// Whether the nav menu should include the "All Photos" entry. Requires both
/// `full_index.generates` and `full_index.show_link`, since a link without a
/// generated target would be broken.
fn show_all_photos_link(config: &SiteConfig) -> bool {
    config.full_index.generates && config.full_index.show_link
}

/// Renders an album page with thumbnail grid
#[allow(clippy::too_many_arguments)]
fn render_album_page(
    album: &Album,
    navigation: &[NavItem],
    pages: &[Page],
    css: &str,
    font_url: Option<&str>,
    site_title: &str,
    favicon_href: Option<&str>,
    snippets: &CustomSnippets,
    show_all_photos: bool,
    og: Option<&OgMeta>,
) -> Markup {
    let nav = render_nav(navigation, &album.path, pages, show_all_photos);

    let segments = path_to_breadcrumb_segments(&album.path, navigation);
    let breadcrumb = html! {
        a href="/" { (site_title) }
        @for (seg_title, seg_path) in &segments {
            " › "
            a href={ "/" (seg_path) "/" } { (seg_title) }
        }
        " › "
        (album.title)
    };

    // The album page is served from `/{album.path}/`, and process-stage image
    // paths are full root-relative (e.g. "travel/japan/001-thumb.avif" for
    // album "travel/japan"). Strip the full album path so URLs resolve
    // against the current directory.
    let album_prefix = format!("{}/", album.path);
    let strip_prefix =
        |path: &str| -> String { path.strip_prefix(&album_prefix).unwrap_or(path).to_string() };

    let has_desc = album.description.is_some();
    let content = html! {
        (site_header(breadcrumb, nav))
        main.album-page.has-description[has_desc] {
            header.album-header {
                h1 { (album.title) }
                @if let Some(desc) = &album.description {
                    input.desc-toggle type="checkbox" id="desc-toggle";
                    div.album-description { (PreEscaped(desc)) }
                    label.desc-expand for="desc-toggle" {
                        span.expand-more { "Read more" }
                        span.expand-less { "Show less" }
                    }
                }
            }
            div.thumbnail-grid {
                @for (idx, image) in album.images.iter().enumerate() {
                    a.thumb-link href=(image_page_url(idx + 1, album.images.len(), image.title.as_deref())) {
                        img src=(strip_prefix(&image.thumbnail)) alt={ "Image " (idx + 1) } loading="lazy";
                    }
                }
            }
        }
    };

    base_document(
        &album.title,
        css,
        font_url,
        None,
        None,
        favicon_href,
        snippets,
        og,
        content,
    )
}

/// Format an image's display label for breadcrumbs and page titles.
///
/// The label is `<index>. <title>` when a title exists, or just `<index>` alone.
///
/// The index is the image's 1-based position in the album (not the sequence
/// number from the filename — ordering can start at any number and be
/// non-contiguous).
///
/// Zero-padding width adapts to the album size:
/// - 1–9 images: no padding (1, 2, 3)
/// - 10–99 images: 2 digits (01, 02, 03)
/// - 100–999 images: 3 digits (001, 002, 003)
/// - 1000+ images: 4 digits (0001, 0002, ...)
fn format_image_label(position: usize, total: usize, title: Option<&str>) -> String {
    let width = index_width(total);
    match title {
        Some(t) => format!("{:0>width$}. {}", position, t),
        None => format!("{:0>width$}", position),
    }
}

/// Renders an image viewer page
#[allow(clippy::too_many_arguments)]
fn render_image_page(
    album: &Album,
    image: &Image,
    prev: Option<&Image>,
    next: Option<&Image>,
    navigation: &[NavItem],
    pages: &[Page],
    css: &str,
    font_url: Option<&str>,
    site_title: &str,
    favicon_href: Option<&str>,
    snippets: &CustomSnippets,
    show_all_photos: bool,
    og: Option<&OgMeta>,
) -> Markup {
    let nav = render_nav(navigation, &album.path, pages, show_all_photos);

    // Image pages live at `/{album.path}/{image_slug}/`, one level below the
    // album directory. Process-stage image paths are full root-relative
    // (e.g. "travel/japan/001-1400.avif" for album "travel/japan"), so strip
    // the full album path and prepend `../` to go up to the album dir.
    let album_prefix = format!("{}/", album.path);
    let strip_prefix = |path: &str| -> String {
        let relative = path.strip_prefix(&album_prefix).unwrap_or(path);
        format!("../{}", relative)
    };

    // Collect variants sorted by width (BTreeMap keys are strings, so lexicographic
    // order doesn't match numeric order — "1400" < "800").
    fn sorted_variants(img: &Image) -> Vec<&GeneratedVariant> {
        let mut v: Vec<_> = img.generated.values().collect();
        v.sort_by_key(|variant| variant.width);
        v
    }

    // Build srcset for a given image's avif variants (ascending width order)
    let avif_srcset_for = |img: &Image| -> String {
        sorted_variants(img)
            .iter()
            .map(|variant| format!("{} {}w", strip_prefix(&variant.avif), variant.width))
            .collect::<Vec<_>>()
            .join(", ")
    };

    // Build srcset
    let variants = sorted_variants(image);

    let srcset_avif: String = avif_srcset_for(image);

    // Use middle size as default
    let default_src = variants
        .get(variants.len() / 2)
        .map(|v| strip_prefix(&v.avif))
        .unwrap_or_default();

    // Pick a single middle-size AVIF URL for adjacent image prefetch
    let mid_avif = |img: &Image| -> String {
        let v = sorted_variants(img);
        v.get(v.len() / 2)
            .map(|variant| strip_prefix(&variant.avif))
            .unwrap_or_default()
    };
    let prev_prefetch = prev.map(&mid_avif);
    let next_prefetch = next.map(&mid_avif);

    // Calculate aspect ratio
    let (width, height) = image.dimensions;
    let aspect_ratio = width as f64 / height as f64;

    // Navigation URLs
    let image_idx = album
        .images
        .iter()
        .position(|i| i.number == image.number)
        .unwrap();

    let total = album.images.len();
    let prev_url = match prev {
        Some(p) => format!(
            "../{}",
            image_page_url(image_idx, total, p.title.as_deref())
        ), // image_idx is 0-based = prev's 1-based
        None => "../".to_string(),
    };

    let next_url = match next {
        Some(n) => format!(
            "../{}",
            image_page_url(image_idx + 2, total, n.title.as_deref())
        ),
        None => "../".to_string(),
    };

    let display_idx = image_idx + 1;
    let image_label = format_image_label(display_idx, album.images.len(), image.title.as_deref());
    let page_title = format!("{} - {}", album.title, image_label);

    let segments = path_to_breadcrumb_segments(&album.path, navigation);
    let breadcrumb = html! {
        a href="/" { (site_title) }
        @for (seg_title, seg_path) in &segments {
            " › "
            a href={ "/" (seg_path) "/" } { (seg_title) }
        }
        " › "
        a href="../" { (album.title) }
        " › "
        (image_label)
    };

    let max_generated_width = image
        .generated
        .values()
        .map(|v| v.width)
        .max()
        .unwrap_or(800);
    let sizes_attr = image_sizes_attr(aspect_ratio, max_generated_width);

    let aspect_style = format!("--aspect-ratio: {};", aspect_ratio);
    let alt_text = match &image.title {
        Some(t) => format!("{} - {}", album.title, t),
        None => format!("{} - Image {}", album.title, display_idx),
    };

    // Build image navigation dot URLs
    let nav_dots: Vec<String> = album
        .images
        .iter()
        .enumerate()
        .map(|(idx, img)| {
            format!(
                "../{}",
                image_page_url(idx + 1, total, img.title.as_deref())
            )
        })
        .collect();

    let description = image.description.as_deref().filter(|d| !d.is_empty());
    let caption_text = description.filter(|d| is_short_caption(d));
    let description_text = description.filter(|d| !is_short_caption(d));

    let body_class = match description {
        Some(desc) if is_short_caption(desc) => "image-view has-caption",
        Some(_) => "image-view has-description",
        None => "image-view",
    };

    // Build <head> extras: render-blocking link + adjacent image prefetches
    let head_extra = html! {
        link rel="expect" href="#main-image" blocking="render";
        @if let Some(ref href) = prev_prefetch {
            link rel="prefetch" as="image" href=(href);
        }
        @if let Some(ref href) = next_prefetch {
            link rel="prefetch" as="image" href=(href);
        }
    };

    let content = html! {
        (site_header(breadcrumb, nav))
        main style=(aspect_style) {
            div.image-page {
                figure.image-frame {
                    img #main-image src=(default_src) srcset=(srcset_avif) sizes=(sizes_attr) alt=(alt_text);
                }
                p.print-credit {
                    (album.title) " › " (image_label)
                }
                @if let Some(text) = caption_text {
                    p.image-caption { (text) }
                }
            }
            @if let Some(text) = description_text {
                div.image-description {
                    p { (text) }
                }
            }
            nav.image-nav {
                @for (idx, url) in nav_dots.iter().enumerate() {
                    @if idx == image_idx {
                        a href=(url) aria-current="true" {}
                    } @else {
                        a href=(url) {}
                    }
                }
            }
            a.nav-prev href=(prev_url) aria-label="Previous image" {}
            a.nav-next href=(next_url) aria-label="Next image" {}
        }
    };

    base_document(
        &page_title,
        css,
        font_url,
        Some(body_class),
        Some(head_extra),
        favicon_href,
        snippets,
        og,
        content,
    )
}

/// Renders a content page from markdown
#[allow(clippy::too_many_arguments)]
fn render_page(
    page: &Page,
    navigation: &[NavItem],
    pages: &[Page],
    css: &str,
    font_url: Option<&str>,
    site_title: &str,
    favicon_href: Option<&str>,
    snippets: &CustomSnippets,
    show_all_photos: bool,
) -> Markup {
    let nav = render_nav(navigation, &page.slug, pages, show_all_photos);

    // Convert markdown to HTML
    let parser = Parser::new(&page.body);
    let mut body_html = String::new();
    md_html::push_html(&mut body_html, parser);

    let breadcrumb = html! {
        a href="/" { (site_title) }
        " › "
        (page.title)
    };

    let content = html! {
        (site_header(breadcrumb, nav))
        main.page {
            article.page-content {
                (PreEscaped(body_html))
            }
        }
    };

    base_document(
        &page.title,
        css,
        font_url,
        None,
        None,
        favicon_href,
        snippets,
        None,
        content,
    )
}

/// Renders a gallery-list page for a container directory (e.g. /NY/).
///
/// Structurally identical to the index page but parameterized: shows a grid
/// of album cards for the container's children.
#[allow(clippy::too_many_arguments)]
fn render_gallery_list_page(
    title: &str,
    path: &str,
    entries: &[GalleryEntry],
    description: Option<&str>,
    navigation: &[NavItem],
    pages: &[Page],
    css: &str,
    font_url: Option<&str>,
    site_title: &str,
    favicon_href: Option<&str>,
    snippets: &CustomSnippets,
    show_all_photos: bool,
    og: Option<&OgMeta>,
) -> Markup {
    let nav = render_nav(navigation, path, pages, show_all_photos);

    let is_root = path.is_empty();
    let segments = path_to_breadcrumb_segments(path, navigation);
    let breadcrumb = html! {
        a href="/" { (site_title) }
        @if !is_root {
            @for (seg_title, seg_path) in &segments {
                " › "
                a href={ "/" (seg_path) "/" } { (seg_title) }
            }
            " › "
            (title)
        }
    };

    let main_class = match description {
        Some(_) => "index-page has-description",
        None => "index-page",
    };
    let content = html! {
        (site_header(breadcrumb, nav))
        main class=(main_class) {
            @if let Some(desc) = description {
                header.index-header {
                    h1 { (title) }
                    input.desc-toggle type="checkbox" id="desc-toggle";
                    div.album-description { (PreEscaped(desc)) }
                    label.desc-expand for="desc-toggle" {
                        span.expand-more { "Read more" }
                        span.expand-less { "Show less" }
                    }
                }
            }
            div.album-grid {
                @for entry in entries {
                    a.album-card href={ "/" (entry.path) "/" } {
                        @if let Some(ref thumb) = entry.thumbnail {
                            img src={ "/" (thumb) } alt=(entry.title) loading="lazy";
                        }
                        span.album-title { (entry.title) }
                    }
                }
            }
        }
    };

    base_document(
        title,
        css,
        font_url,
        None,
        None,
        favicon_href,
        snippets,
        og,
        content,
    )
}

/// Renders the site-wide "All Photos" page — a single thumbnail grid containing
/// every image from every public (numbered) album. Uses the full-index thumbnails
/// generated in the process stage, with gap and aspect ratio controlled by
/// `[full_index]` config. Each thumbnail links to the image's normal page.
fn render_full_index_page(
    manifest: &Manifest,
    css: &str,
    font_url: Option<&str>,
    favicon_href: Option<&str>,
    snippets: &CustomSnippets,
) -> Markup {
    let title = "All Photos";
    let path = "all-photos";
    let fi = &manifest.config.full_index;

    let nav = render_nav(
        &manifest.navigation,
        path,
        &manifest.pages,
        show_all_photos_link(&manifest.config),
    );

    let breadcrumb = html! {
        a href="/" { (manifest.config.site_title) }
        " › "
        (title)
    };

    // Collect entries: every image from every in-nav album, in album order.
    struct FullIndexEntry<'a> {
        thumbnail: String,
        link: String,
        alt: String,
        #[allow(dead_code)]
        album_title: &'a str,
    }

    let mut entries: Vec<FullIndexEntry> = Vec::new();
    for album in &manifest.albums {
        if !album.in_nav {
            continue;
        }
        let total = album.images.len();
        for (idx, image) in album.images.iter().enumerate() {
            let Some(ref thumb) = image.full_index_thumbnail else {
                continue;
            };
            let image_dir = image_page_url(idx + 1, total, image.title.as_deref());
            let link = format!("/{}/{}", album.path, image_dir);
            let alt = match &image.title {
                Some(t) => format!("{} - {}", album.title, t),
                None => format!("{} - Image {}", album.title, idx + 1),
            };
            entries.push(FullIndexEntry {
                thumbnail: format!("/{}", thumb),
                link,
                alt,
                album_title: &album.title,
            });
        }
    }

    // Inline CSS variables: custom gap, aspect ratio, and grid column width
    // just for this page, so a site can tune the full-index grid independently
    // from album grids.
    //
    // The displayed column width is derived from thumb_ratio + thumb_size so
    // the CSS grid cells match the pixel dimensions of the generated thumbnail.
    // Without this, the grid falls back to the album-grid minmax(200px, 1fr)
    // and a source shrunk to 100px (or stretched from 100px to 200px) would
    // look blurry regardless of thumb_size.
    //
    // thumb_size is the short-edge size; long edge scales by the ratio.
    let (rw, rh) = (fi.thumb_ratio[0].max(1), fi.thumb_ratio[1].max(1));
    let display_width_px = if rw >= rh {
        (fi.thumb_size as f64 * rw as f64 / rh as f64).round() as u32
    } else {
        fi.thumb_size
    };
    let aspect_ratio_css = format!("{} / {}", rw, rh);
    let main_style = format!(
        "--thumbnail-gap: {}; --fi-thumb-aspect: {}; --fi-thumb-col-width: {}px;",
        fi.thumb_gap, aspect_ratio_css, display_width_px
    );

    let content = html! {
        (site_header(breadcrumb, nav))
        main.album-page.full-index-page style=(main_style) {
            header.album-header {
                h1 { (title) }
            }
            div.thumbnail-grid {
                @for entry in &entries {
                    a.thumb-link href=(entry.link) {
                        img src=(entry.thumbnail) alt=(entry.alt) loading="lazy";
                    }
                }
            }
        }
    };

    base_document(
        title,
        css,
        font_url,
        None,
        None,
        favicon_href,
        snippets,
        None,
        content,
    )
}

/// Walk the navigation tree and generate gallery-list pages for every container.
#[allow(clippy::too_many_arguments)]
fn generate_gallery_list_pages(
    items: &[NavItem],
    albums: &[Album],
    navigation: &[NavItem],
    pages: &[Page],
    css: &str,
    font_url: Option<&str>,
    site_title: &str,
    favicon_href: Option<&str>,
    snippets: &CustomSnippets,
    show_all_photos: bool,
    base_url: Option<&str>,
    output_dir: &Path,
) -> Result<(), GenerateError> {
    for item in items {
        if !item.children.is_empty() {
            let entries = collect_gallery_entries(&item.children, albums);
            let og = base_url.and_then(|base| {
                build_og_for_gallery_list(
                    base,
                    &item.title,
                    &item.path,
                    &item.children,
                    navigation,
                    albums,
                    site_title,
                )
            });
            let page_html = render_gallery_list_page(
                &item.title,
                &item.path,
                &entries,
                item.description.as_deref(),
                navigation,
                pages,
                css,
                font_url,
                site_title,
                favicon_href,
                snippets,
                show_all_photos,
                og.as_ref(),
            );
            let dir = output_dir.join(&item.path);
            fs::create_dir_all(&dir)?;
            fs::write(dir.join("index.html"), page_html.into_string())?;

            // Recurse into children
            generate_gallery_list_pages(
                &item.children,
                albums,
                navigation,
                pages,
                css,
                font_url,
                site_title,
                favicon_href,
                snippets,
                show_all_photos,
                base_url,
                output_dir,
            )?;
        }
    }
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn no_snippets() -> CustomSnippets {
        CustomSnippets::default()
    }

    fn make_page(slug: &str, link_title: &str, in_nav: bool, is_link: bool) -> Page {
        Page {
            title: link_title.to_string(),
            link_title: link_title.to_string(),
            slug: slug.to_string(),
            body: if is_link {
                "https://example.com".to_string()
            } else {
                format!("# {}\n\nContent.", link_title)
            },
            in_nav,
            sort_key: if in_nav { 40 } else { u32::MAX },
            is_link,
        }
    }

    #[test]
    fn nav_renders_items() {
        let items = vec![NavItem {
            title: "Album One".to_string(),
            path: "010-one".to_string(),
            source_dir: String::new(),
            description: None,
            children: vec![],
        }];
        let html = render_nav(&items, "", &[], false).into_string();
        assert!(html.contains("Album One"));
        assert!(html.contains("/010-one/"));
    }

    #[test]
    fn nav_includes_pages() {
        let pages = vec![make_page("about", "About", true, false)];
        let html = render_nav(&[], "", &pages, false).into_string();
        assert!(html.contains("About"));
        assert!(html.contains("/about.html"));
    }

    #[test]
    fn nav_hides_unnumbered_pages() {
        let pages = vec![make_page("notes", "Notes", false, false)];
        let html = render_nav(&[], "", &pages, false).into_string();
        assert!(!html.contains("Notes"));
        // No separator either when no nav pages
        assert!(!html.contains("nav-separator"));
    }

    #[test]
    fn nav_renders_link_page_as_external() {
        let pages = vec![make_page("github", "GitHub", true, true)];
        let html = render_nav(&[], "", &pages, false).into_string();
        assert!(html.contains("GitHub"));
        assert!(html.contains("https://example.com"));
        assert!(html.contains("target=\"_blank\""));
    }

    #[test]
    fn nav_marks_current_item() {
        let items = vec![
            NavItem {
                title: "First".to_string(),
                path: "010-first".to_string(),
                source_dir: String::new(),
                description: None,
                children: vec![],
            },
            NavItem {
                title: "Second".to_string(),
                path: "020-second".to_string(),
                source_dir: String::new(),
                description: None,
                children: vec![],
            },
        ];
        let html = render_nav(&items, "020-second", &[], false).into_string();
        // The second item should have the current class
        assert!(html.contains(r#"class="current"#));
    }

    #[test]
    fn nav_marks_current_page() {
        let pages = vec![make_page("about", "About", true, false)];
        let html = render_nav(&[], "about", &pages, false).into_string();
        assert!(html.contains(r#"class="current"#));
    }

    #[test]
    fn nav_renders_nested_children() {
        let items = vec![NavItem {
            title: "Parent".to_string(),
            path: "010-parent".to_string(),
            source_dir: String::new(),
            description: None,
            children: vec![NavItem {
                title: "Child".to_string(),
                path: "010-parent/010-child".to_string(),
                source_dir: String::new(),
                description: None,
                children: vec![],
            }],
        }];
        let html = render_nav(&items, "", &[], false).into_string();
        assert!(html.contains("Parent"));
        assert!(html.contains("Child"));
        assert!(html.contains("nav-group")); // Parent should have nav-group class
    }

    #[test]
    fn nav_separator_only_when_pages() {
        // No pages = no separator
        let html_no_pages = render_nav(&[], "", &[], false).into_string();
        assert!(!html_no_pages.contains("nav-separator"));

        // With nav pages = separator
        let pages = vec![make_page("about", "About", true, false)];
        let html_with_pages = render_nav(&[], "", &pages, false).into_string();
        assert!(html_with_pages.contains("nav-separator"));
    }

    #[test]
    fn base_document_includes_doctype() {
        let content = html! { p { "test" } };
        let doc = base_document(
            "Test",
            "body {}",
            None,
            None,
            None,
            None,
            &no_snippets(),
            None,
            content,
        )
        .into_string();
        assert!(doc.starts_with("<!DOCTYPE html>"));
    }

    #[test]
    fn base_document_applies_body_class() {
        let content = html! { p { "test" } };
        let doc = base_document(
            "Test",
            "",
            None,
            Some("image-view"),
            None,
            None,
            &no_snippets(),
            None,
            content,
        )
        .into_string();
        assert!(html_contains_body_class(&doc, "image-view"));
    }

    #[test]
    fn site_header_structure() {
        let breadcrumb = html! { a href="/" { "Home" } };
        let nav = html! { ul { li { "Item" } } };
        let header = site_header(breadcrumb, nav).into_string();

        assert!(header.contains("site-header"));
        assert!(header.contains("breadcrumb"));
        assert!(header.contains("site-nav"));
        assert!(header.contains("Home"));
    }

    // Helper to check if body has a specific class
    fn html_contains_body_class(html: &str, class: &str) -> bool {
        // Look for body tag with class attribute containing the class
        html.contains(&format!(r#"class="{}""#, class))
    }

    // =========================================================================
    // Page renderer tests
    // =========================================================================

    fn create_test_album() -> Album {
        Album {
            path: "test".to_string(),
            title: "Test Album".to_string(),
            description: Some("<p>A test album description</p>".to_string()),
            thumbnail: "test/001-image-thumb.avif".to_string(),
            images: vec![
                Image {
                    number: 1,
                    source_path: "test/001-dawn.jpg".to_string(),
                    title: Some("Dawn".to_string()),
                    description: None,
                    dimensions: (1600, 1200),
                    generated: {
                        let mut map = BTreeMap::new();
                        map.insert(
                            "800".to_string(),
                            GeneratedVariant {
                                avif: "test/001-dawn-800.avif".to_string(),
                                width: 800,
                                height: 600,
                            },
                        );
                        map.insert(
                            "1400".to_string(),
                            GeneratedVariant {
                                avif: "test/001-dawn-1400.avif".to_string(),
                                width: 1400,
                                height: 1050,
                            },
                        );
                        map
                    },
                    thumbnail: "test/001-dawn-thumb.avif".to_string(),
                    full_index_thumbnail: None,
                },
                Image {
                    number: 2,
                    source_path: "test/002-night.jpg".to_string(),
                    title: None,
                    description: None,
                    dimensions: (1200, 1600),
                    generated: {
                        let mut map = BTreeMap::new();
                        map.insert(
                            "800".to_string(),
                            GeneratedVariant {
                                avif: "test/002-night-800.avif".to_string(),
                                width: 600,
                                height: 800,
                            },
                        );
                        map
                    },
                    thumbnail: "test/002-night-thumb.avif".to_string(),
                    full_index_thumbnail: None,
                },
            ],
            in_nav: true,
            config: SiteConfig::default(),
            support_files: vec![],
        }
    }

    /// A nested album (e.g. `NY/Night`) shaped the way the process stage
    /// actually emits records: every image path is **full root-relative**
    /// (`NY/Night/001-city-800.avif`), matching what ends up in
    /// `{temp_dir}/processed/manifest.json` for real builds.
    fn create_nested_test_album() -> Album {
        Album {
            path: "NY/Night".to_string(),
            title: "Night".to_string(),
            description: None,
            thumbnail: "NY/Night/001-city-thumb.avif".to_string(),
            images: vec![Image {
                number: 1,
                source_path: "NY/Night/001-city.jpg".to_string(),
                title: Some("City".to_string()),
                description: None,
                dimensions: (1600, 1200),
                generated: {
                    let mut map = BTreeMap::new();
                    map.insert(
                        "800".to_string(),
                        GeneratedVariant {
                            avif: "NY/Night/001-city-800.avif".to_string(),
                            width: 800,
                            height: 600,
                        },
                    );
                    map.insert(
                        "1400".to_string(),
                        GeneratedVariant {
                            avif: "NY/Night/001-city-1400.avif".to_string(),
                            width: 1400,
                            height: 1050,
                        },
                    );
                    map
                },
                thumbnail: "NY/Night/001-city-thumb.avif".to_string(),
                full_index_thumbnail: None,
            }],
            in_nav: true,
            config: SiteConfig::default(),
            support_files: vec![],
        }
    }

    #[test]
    fn nested_album_thumbnail_paths_are_relative_to_album_dir() {
        let album = create_nested_test_album();
        let html = render_album_page(
            &album,
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        // The album page is served from `/NY/Night/`, so `<img src>` must be
        // a bare filename. Any `NY/` or `NY/Night/` prefix in `src=` would
        // resolve to `/NY/Night/NY/...` — a broken path.
        assert!(html.contains(r#"src="001-city-thumb.avif""#));
        assert!(!html.contains(r#"src="NY/"#));
    }

    #[test]
    fn nested_album_image_page_srcset_paths_are_relative() {
        let album = create_nested_test_album();
        let image = &album.images[0];
        let html = render_image_page(
            &album,
            image,
            None,
            None,
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        // Image page is at `/NY/Night/1-city/`, so srcset paths use `../` to
        // reach the album directory. Must be bare filenames after `../` — any
        // `../NY/` or `../Night/` would resolve to a doubled album prefix.
        assert!(html.contains("../001-city-800.avif"));
        assert!(html.contains("../001-city-1400.avif"));
        assert!(!html.contains("../NY/"));
        assert!(!html.contains("../Night/"));
    }

    #[test]
    fn render_album_page_includes_title() {
        let album = create_test_album();
        let nav = vec![];
        let html = render_album_page(
            &album,
            &nav,
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(html.contains("Test Album"));
        assert!(html.contains("<h1>"));
    }

    #[test]
    fn render_album_page_includes_description() {
        let album = create_test_album();
        let nav = vec![];
        let html = render_album_page(
            &album,
            &nav,
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(html.contains("A test album description"));
        assert!(html.contains("album-description"));
    }

    #[test]
    fn render_album_page_thumbnail_links() {
        let album = create_test_album();
        let nav = vec![];
        let html = render_album_page(
            &album,
            &nav,
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        // Should have links to image pages (1-dawn/, 2/)
        assert!(html.contains("1-dawn/"));
        assert!(html.contains("2/"));
        // Thumbnails should have paths relative to album dir
        assert!(html.contains("001-dawn-thumb.avif"));
    }

    #[test]
    fn render_album_page_breadcrumb() {
        let album = create_test_album();
        let nav = vec![];
        let html = render_album_page(
            &album,
            &nav,
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        // Breadcrumb should link to gallery root
        assert!(html.contains(r#"href="/""#));
        assert!(html.contains("Gallery"));
    }

    #[test]
    fn render_image_page_includes_img_with_srcset() {
        let album = create_test_album();
        let image = &album.images[0];
        let nav = vec![];
        let html = render_image_page(
            &album,
            image,
            None,
            Some(&album.images[1]),
            &nav,
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(html.contains("<img"));
        assert!(html.contains("srcset="));
        assert!(html.contains(".avif"));
        assert!(!html.contains("<picture>"));
    }

    #[test]
    fn render_image_page_srcset() {
        let album = create_test_album();
        let image = &album.images[0];
        let nav = vec![];
        let html = render_image_page(
            &album,
            image,
            None,
            Some(&album.images[1]),
            &nav,
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        // Should have srcset with sizes
        assert!(html.contains("srcset="));
        assert!(html.contains("800w"));
        assert!(html.contains("1400w"));
    }

    #[test]
    fn render_image_page_nav_links() {
        let album = create_test_album();
        let image = &album.images[0];
        let nav = vec![];
        let html = render_image_page(
            &album,
            image,
            None,
            Some(&album.images[1]),
            &nav,
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(html.contains("nav-prev"));
        assert!(html.contains("nav-next"));
        assert!(html.contains(r#"aria-label="Previous image""#));
        assert!(html.contains(r#"aria-label="Next image""#));
    }

    #[test]
    fn render_image_page_prev_next_urls() {
        let album = create_test_album();
        let nav = vec![];

        // First image - no prev, has next
        let html1 = render_image_page(
            &album,
            &album.images[0],
            None,
            Some(&album.images[1]),
            &nav,
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();
        assert!(html1.contains(r#"class="nav-prev" href="../""#));
        assert!(html1.contains(r#"class="nav-next" href="../2/""#));

        // Second image - has prev, no next (image[1] has no title)
        let html2 = render_image_page(
            &album,
            &album.images[1],
            Some(&album.images[0]),
            None,
            &nav,
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();
        assert!(html2.contains(r#"class="nav-prev" href="../1-dawn/""#));
        assert!(html2.contains(r#"class="nav-next" href="../""#));
    }

    #[test]
    fn render_image_page_aspect_ratio() {
        let album = create_test_album();
        let image = &album.images[0]; // 1600x1200 = 1.333...
        let nav = vec![];
        let html = render_image_page(
            &album,
            image,
            None,
            None,
            &nav,
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        // Should have aspect ratio CSS variable
        assert!(html.contains("--aspect-ratio:"));
    }

    #[test]
    fn render_page_converts_markdown() {
        let page = Page {
            title: "About Me".to_string(),
            link_title: "about".to_string(),
            slug: "about".to_string(),
            body: "# About Me\n\nThis is **bold** and *italic*.".to_string(),
            in_nav: true,
            sort_key: 40,
            is_link: false,
        };
        let html = render_page(
            &page,
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
        )
        .into_string();

        // Markdown should be converted to HTML
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<em>italic</em>"));
    }

    #[test]
    fn render_page_includes_title() {
        let page = Page {
            title: "About Me".to_string(),
            link_title: "about me".to_string(),
            slug: "about".to_string(),
            body: "Content here".to_string(),
            in_nav: true,
            sort_key: 40,
            is_link: false,
        };
        let html = render_page(
            &page,
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
        )
        .into_string();

        assert!(html.contains("<title>About Me</title>"));
        assert!(html.contains("class=\"page\""));
    }

    // =========================================================================
    // Image label and breadcrumb tests
    // =========================================================================

    #[test]
    fn format_label_with_title() {
        assert_eq!(format_image_label(1, 5, Some("Museum")), "1. Museum");
    }

    #[test]
    fn format_label_without_title() {
        assert_eq!(format_image_label(1, 5, None), "1");
    }

    #[test]
    fn format_label_zero_pads_for_10_plus() {
        assert_eq!(format_image_label(3, 15, Some("Dawn")), "03. Dawn");
        assert_eq!(format_image_label(3, 15, None), "03");
    }

    #[test]
    fn format_label_zero_pads_for_100_plus() {
        assert_eq!(format_image_label(7, 120, Some("X")), "007. X");
        assert_eq!(format_image_label(7, 120, None), "007");
    }

    #[test]
    fn format_label_no_padding_under_10() {
        assert_eq!(format_image_label(3, 9, Some("Y")), "3. Y");
    }

    #[test]
    fn image_breadcrumb_includes_title() {
        let album = create_test_album();
        let image = &album.images[0]; // has title "Dawn"
        let nav = vec![];
        let html = render_image_page(
            &album,
            image,
            None,
            Some(&album.images[1]),
            &nav,
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        // Breadcrumb: Gallery › Test Album › 1. Dawn
        assert!(html.contains("1. Dawn"));
        assert!(html.contains("Test Album"));
    }

    #[test]
    fn image_breadcrumb_without_title() {
        let album = create_test_album();
        let image = &album.images[1]; // no title
        let nav = vec![];
        let html = render_image_page(
            &album,
            image,
            Some(&album.images[0]),
            None,
            &nav,
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        // Breadcrumb: Gallery › Test Album › 2
        assert!(html.contains("Test Album"));
        // Should contain just "2" without a period
        assert!(html.contains(" › 2<"));
    }

    #[test]
    fn image_page_title_includes_label() {
        let album = create_test_album();
        let image = &album.images[0];
        let nav = vec![];
        let html = render_image_page(
            &album,
            image,
            None,
            Some(&album.images[1]),
            &nav,
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(html.contains("<title>Test Album - 1. Dawn</title>"));
    }

    #[test]
    fn image_alt_text_uses_title() {
        let album = create_test_album();
        let image = &album.images[0]; // has title "Dawn"
        let nav = vec![];
        let html = render_image_page(
            &album,
            image,
            None,
            Some(&album.images[1]),
            &nav,
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(html.contains("Test Album - Dawn"));
    }

    // =========================================================================
    // Description detection and rendering tests
    // =========================================================================

    #[test]
    fn is_short_caption_short_text() {
        assert!(is_short_caption("A beautiful sunset"));
    }

    #[test]
    fn is_short_caption_exactly_at_limit() {
        let text = "a".repeat(SHORT_CAPTION_MAX_LEN);
        assert!(is_short_caption(&text));
    }

    #[test]
    fn is_short_caption_over_limit() {
        let text = "a".repeat(SHORT_CAPTION_MAX_LEN + 1);
        assert!(!is_short_caption(&text));
    }

    #[test]
    fn is_short_caption_with_newline() {
        assert!(!is_short_caption("Line one\nLine two"));
    }

    #[test]
    fn is_short_caption_empty_string() {
        assert!(is_short_caption(""));
    }

    #[test]
    fn render_image_page_short_caption() {
        let mut album = create_test_album();
        album.images[0].description = Some("A beautiful sunrise over the mountains".to_string());
        let image = &album.images[0];
        let html = render_image_page(
            &album,
            image,
            None,
            Some(&album.images[1]),
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(html.contains("image-caption"));
        assert!(html.contains("A beautiful sunrise over the mountains"));
        assert!(html_contains_body_class(&html, "image-view has-caption"));
    }

    #[test]
    fn render_image_page_long_description() {
        let mut album = create_test_album();
        let long_text = "a".repeat(200);
        album.images[0].description = Some(long_text.clone());
        let image = &album.images[0];
        let html = render_image_page(
            &album,
            image,
            None,
            Some(&album.images[1]),
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(html.contains("image-description"));
        assert!(!html.contains("image-caption"));
        assert!(html_contains_body_class(
            &html,
            "image-view has-description"
        ));
    }

    #[test]
    fn render_image_page_multiline_is_long_description() {
        let mut album = create_test_album();
        album.images[0].description = Some("Line one\nLine two".to_string());
        let image = &album.images[0];
        let html = render_image_page(
            &album,
            image,
            None,
            Some(&album.images[1]),
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(html.contains("image-description"));
        assert!(!html.contains("image-caption"));
        assert!(html_contains_body_class(
            &html,
            "image-view has-description"
        ));
    }

    #[test]
    fn render_image_page_no_description_no_caption() {
        let album = create_test_album();
        let image = &album.images[1]; // description: None
        let html = render_image_page(
            &album,
            image,
            Some(&album.images[0]),
            None,
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(!html.contains("image-caption"));
        assert!(!html.contains("image-description"));
        assert!(html_contains_body_class(&html, "image-view"));
    }

    #[test]
    fn render_image_page_caption_width_matches_frame() {
        let mut album = create_test_album();
        album.images[0].description = Some("Short caption".to_string());
        let image = &album.images[0];
        let html = render_image_page(
            &album,
            image,
            None,
            Some(&album.images[1]),
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        // Caption should be a sibling of image-frame inside image-page
        assert!(html.contains("image-frame"));
        assert!(html.contains("image-caption"));
        // Both should be inside image-page (column flex ensures width matching via CSS)
        assert!(html.contains("image-page"));
    }

    #[test]
    fn html_escape_in_maud() {
        // Maud should automatically escape HTML in content
        let items = vec![NavItem {
            title: "<script>alert('xss')</script>".to_string(),
            path: "test".to_string(),
            source_dir: String::new(),
            description: None,
            children: vec![],
        }];
        let html = render_nav(&items, "", &[], false).into_string();

        // Should be escaped, not raw script tag
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;script&gt;"));
    }

    // =========================================================================
    // escape_for_url tests
    // =========================================================================

    #[test]
    fn escape_for_url_spaces_become_dashes() {
        assert_eq!(escape_for_url("My Title"), "my-title");
    }

    #[test]
    fn escape_for_url_dots_become_dashes() {
        assert_eq!(escape_for_url("St. Louis"), "st-louis");
    }

    #[test]
    fn escape_for_url_collapses_consecutive() {
        assert_eq!(escape_for_url("A.  B"), "a-b");
    }

    #[test]
    fn escape_for_url_strips_leading_trailing() {
        assert_eq!(escape_for_url(". Title ."), "title");
    }

    #[test]
    fn escape_for_url_preserves_dashes() {
        assert_eq!(escape_for_url("My-Title"), "my-title");
    }

    #[test]
    fn escape_for_url_underscores_become_dashes() {
        assert_eq!(escape_for_url("My_Title"), "my-title");
    }

    #[test]
    fn image_page_url_with_title() {
        assert_eq!(image_page_url(3, 15, Some("Dawn")), "03-dawn/");
    }

    #[test]
    fn image_page_url_without_title() {
        assert_eq!(image_page_url(3, 15, None), "03/");
    }

    #[test]
    fn image_page_url_title_with_spaces() {
        assert_eq!(image_page_url(1, 5, Some("My Museum")), "1-my-museum/");
    }

    #[test]
    fn image_page_url_title_with_dot() {
        assert_eq!(image_page_url(1, 5, Some("St. Louis")), "1-st-louis/");
    }

    // =========================================================================
    // View transition: render-blocking and image preload tests
    // =========================================================================

    #[test]
    fn render_image_page_has_main_image_id() {
        let album = create_test_album();
        let image = &album.images[0];
        let html = render_image_page(
            &album,
            image,
            None,
            Some(&album.images[1]),
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(html.contains(r#"id="main-image""#));
    }

    #[test]
    fn render_image_page_has_render_blocking_link() {
        let album = create_test_album();
        let image = &album.images[0];
        let html = render_image_page(
            &album,
            image,
            None,
            Some(&album.images[1]),
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(html.contains(r#"rel="expect""#));
        assert!(html.contains(r##"href="#main-image""##));
        assert!(html.contains(r#"blocking="render""#));
    }

    #[test]
    fn render_image_page_prefetches_next_image() {
        let album = create_test_album();
        let image = &album.images[0];
        let html = render_image_page(
            &album,
            image,
            None,
            Some(&album.images[1]),
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        // Should have a prefetch link with the next image's middle-size avif
        assert!(html.contains(r#"rel="prefetch""#));
        assert!(html.contains(r#"as="image""#));
        assert!(html.contains("002-night-800.avif"));
    }

    #[test]
    fn render_image_page_prefetches_prev_image() {
        let album = create_test_album();
        let image = &album.images[1];
        let html = render_image_page(
            &album,
            image,
            Some(&album.images[0]),
            None,
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        // Should have a prefetch link with the prev image's middle-size avif
        // Variants sorted by width: [800, 1400], middle (index 1) = 1400
        assert!(html.contains(r#"rel="prefetch""#));
        assert!(html.contains("001-dawn-1400.avif"));
        // Single URL (href), not a srcset — should not contain both sizes
        assert!(!html.contains("001-dawn-800.avif"));
    }

    #[test]
    fn render_image_page_no_prefetch_without_adjacent() {
        let album = create_test_album();
        let image = &album.images[0];
        // No prev, no next
        let html = render_image_page(
            &album,
            image,
            None,
            None,
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        // Should still have the render-blocking link
        assert!(html.contains(r#"rel="expect""#));
        // Should NOT have any prefetch links
        assert!(!html.contains(r#"rel="prefetch""#));
    }

    // =========================================================================
    // CSS variables from config in rendered HTML
    // =========================================================================

    #[test]
    fn rendered_html_contains_color_css_variables() {
        let mut config = SiteConfig::default();
        config.colors.light.background = "#fafafa".to_string();
        config.colors.dark.background = "#111111".to_string();

        let color_css = crate::config::generate_color_css(&config.colors);
        let theme_css = crate::config::generate_theme_css(&config.theme);
        let font_css = crate::config::generate_font_css(&config.font);
        let css = format!("{}\n{}\n{}", color_css, theme_css, font_css);

        let album = create_test_album();
        let html = render_album_page(
            &album,
            &[],
            &[],
            &css,
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(html.contains("--color-bg: #fafafa"));
        assert!(html.contains("--color-bg: #111111"));
        assert!(html.contains("--color-text:"));
        assert!(html.contains("--color-text-muted:"));
        assert!(html.contains("--color-border:"));
        assert!(html.contains("--color-link:"));
        assert!(html.contains("--color-link-hover:"));
    }

    #[test]
    fn rendered_html_contains_theme_css_variables() {
        let mut config = SiteConfig::default();
        config.theme.thumbnail_gap = "0.5rem".to_string();
        config.theme.mat_x.size = "5vw".to_string();

        let theme_css = crate::config::generate_theme_css(&config.theme);
        let album = create_test_album();
        let html = render_album_page(
            &album,
            &[],
            &[],
            &theme_css,
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(html.contains("--thumbnail-gap: 0.5rem"));
        assert!(html.contains("--mat-x: clamp(1rem, 5vw, 2.5rem)"));
        assert!(html.contains("--mat-y:"));
        assert!(html.contains("--grid-padding:"));
    }

    #[test]
    fn rendered_html_contains_font_css_variables() {
        let mut config = SiteConfig::default();
        config.font.font = "Lora".to_string();
        config.font.weight = "300".to_string();
        config.font.font_type = crate::config::FontType::Serif;

        let font_css = crate::config::generate_font_css(&config.font);
        let font_url = config.font.stylesheet_url();

        let album = create_test_album();
        let html = render_album_page(
            &album,
            &[],
            &[],
            &font_css,
            font_url.as_deref(),
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(html.contains("--font-family:"));
        assert!(html.contains("--font-weight: 300"));
        assert!(html.contains("fonts.googleapis.com"));
        assert!(html.contains("Lora"));
    }

    // =========================================================================
    // Index page edge cases
    // =========================================================================

    #[test]
    fn index_page_excludes_non_nav_albums() {
        let manifest = Manifest {
            navigation: vec![NavItem {
                title: "Visible".to_string(),
                path: "visible".to_string(),
                source_dir: String::new(),
                description: None,
                children: vec![],
            }],
            albums: vec![
                Album {
                    path: "visible".to_string(),
                    title: "Visible".to_string(),
                    description: None,
                    thumbnail: "visible/thumb.avif".to_string(),
                    images: vec![],
                    in_nav: true,
                    config: SiteConfig::default(),
                    support_files: vec![],
                },
                Album {
                    path: "hidden".to_string(),
                    title: "Hidden".to_string(),
                    description: None,
                    thumbnail: "hidden/thumb.avif".to_string(),
                    images: vec![],
                    in_nav: false,
                    config: SiteConfig::default(),
                    support_files: vec![],
                },
            ],
            pages: vec![],
            description: None,
            config: SiteConfig::default(),
        };

        let html = render_index(&manifest, "", None, None, &no_snippets(), None).into_string();

        assert!(html.contains("Visible"));
        assert!(!html.contains("Hidden"));
    }

    // =========================================================================
    // Full-index ("All Photos") tests
    // =========================================================================

    fn make_full_index_manifest() -> Manifest {
        // Two public albums with one image each. Each image has a full-index
        // thumbnail populated as if the process stage ran with generates=true.
        let mut cfg = SiteConfig::default();
        cfg.full_index.generates = true;
        cfg.full_index.show_link = true;
        cfg.full_index.thumb_ratio = [4, 4];
        cfg.full_index.thumb_size = 1000;
        cfg.full_index.thumb_gap = "0.5rem".to_string();

        let make_image = |album: &str, n: u32, slug: &str, title: &str| Image {
            number: n,
            source_path: format!("{}/00{}-{}.jpg", album, n, slug),
            title: Some(title.to_string()),
            description: None,
            dimensions: (1600, 1200),
            generated: {
                let mut map = BTreeMap::new();
                map.insert(
                    "800".to_string(),
                    GeneratedVariant {
                        avif: format!("{}/00{}-{}-800.avif", album, n, slug),
                        width: 800,
                        height: 600,
                    },
                );
                map
            },
            thumbnail: format!("{}/00{}-{}-thumb.avif", album, n, slug),
            full_index_thumbnail: Some(format!("{}/00{}-{}-fi-thumb.avif", album, n, slug)),
        };

        Manifest {
            navigation: vec![
                NavItem {
                    title: "Alpha".to_string(),
                    path: "alpha".to_string(),
                    source_dir: "010-Alpha".to_string(),
                    description: None,
                    children: vec![],
                },
                NavItem {
                    title: "Beta".to_string(),
                    path: "beta".to_string(),
                    source_dir: "020-Beta".to_string(),
                    description: None,
                    children: vec![],
                },
            ],
            albums: vec![
                Album {
                    path: "alpha".to_string(),
                    title: "Alpha".to_string(),
                    description: None,
                    thumbnail: "alpha/001-dawn-thumb.avif".to_string(),
                    images: vec![make_image("alpha", 1, "dawn", "Dawn")],
                    in_nav: true,
                    config: cfg.clone(),
                    support_files: vec![],
                },
                Album {
                    path: "beta".to_string(),
                    title: "Beta".to_string(),
                    description: None,
                    thumbnail: "beta/001-dusk-thumb.avif".to_string(),
                    images: vec![make_image("beta", 1, "dusk", "Dusk")],
                    in_nav: true,
                    config: cfg.clone(),
                    support_files: vec![],
                },
            ],
            pages: vec![],
            description: None,
            config: cfg,
        }
    }

    #[test]
    fn full_index_page_contains_every_image() {
        let manifest = make_full_index_manifest();
        let html = render_full_index_page(&manifest, "", None, None, &no_snippets()).into_string();

        assert!(html.contains("All Photos"));
        assert!(html.contains("full-index-page"));
        assert!(html.contains("/alpha/001-dawn-fi-thumb.avif"));
        assert!(html.contains("/beta/001-dusk-fi-thumb.avif"));
        // Each thumbnail should link to the image's normal page.
        assert!(html.contains(r#"href="/alpha/1-dawn/""#));
        assert!(html.contains(r#"href="/beta/1-dusk/""#));
    }

    #[test]
    fn full_index_page_applies_thumb_gap_and_aspect() {
        let manifest = make_full_index_manifest();
        let html = render_full_index_page(&manifest, "", None, None, &no_snippets()).into_string();

        // Inline CSS vars on <main> let a site tune the grid independently.
        assert!(html.contains("--thumbnail-gap: 0.5rem"));
        assert!(html.contains("--fi-thumb-aspect: 4 / 4"));
    }

    #[test]
    fn full_index_page_column_width_square_matches_thumb_size() {
        // thumb_ratio = [4, 4] (square), thumb_size = 1000 → col width = 1000px
        let manifest = make_full_index_manifest();
        let html = render_full_index_page(&manifest, "", None, None, &no_snippets()).into_string();
        assert!(html.contains("--fi-thumb-col-width: 1000px"));
    }

    #[test]
    fn full_index_page_column_width_portrait_uses_short_edge() {
        // Portrait [4, 5] thumb_size=400 → width = 400 (short edge)
        let mut manifest = make_full_index_manifest();
        manifest.config.full_index.thumb_ratio = [4, 5];
        manifest.config.full_index.thumb_size = 400;
        let html = render_full_index_page(&manifest, "", None, None, &no_snippets()).into_string();
        assert!(html.contains("--fi-thumb-col-width: 400px"));
    }

    #[test]
    fn full_index_page_column_width_landscape_scales_by_ratio() {
        // Landscape [16, 9] thumb_size=400 → width = 400 * 16 / 9 ≈ 711
        let mut manifest = make_full_index_manifest();
        manifest.config.full_index.thumb_ratio = [16, 9];
        manifest.config.full_index.thumb_size = 400;
        let html = render_full_index_page(&manifest, "", None, None, &no_snippets()).into_string();
        assert!(html.contains("--fi-thumb-col-width: 711px"));
    }

    #[test]
    fn full_index_page_excludes_hidden_albums() {
        let mut manifest = make_full_index_manifest();
        // Add a hidden album whose image must NOT appear on the All Photos page.
        let hidden = Album {
            path: "hidden".to_string(),
            title: "Hidden".to_string(),
            description: None,
            thumbnail: "hidden/001-secret-thumb.avif".to_string(),
            images: vec![Image {
                number: 1,
                source_path: "hidden/001-secret.jpg".to_string(),
                title: Some("Secret".to_string()),
                description: None,
                dimensions: (1600, 1200),
                generated: BTreeMap::new(),
                thumbnail: "hidden/001-secret-thumb.avif".to_string(),
                full_index_thumbnail: Some("hidden/001-secret-fi-thumb.avif".to_string()),
            }],
            in_nav: false,
            config: manifest.config.clone(),
            support_files: vec![],
        };
        manifest.albums.push(hidden);

        let html = render_full_index_page(&manifest, "", None, None, &no_snippets()).into_string();

        assert!(!html.contains("secret-fi-thumb"));
        assert!(!html.contains("/hidden/"));
    }

    #[test]
    fn all_photos_nav_link_appears_when_enabled() {
        let mut cfg = SiteConfig::default();
        cfg.full_index.generates = true;
        cfg.full_index.show_link = true;
        let html = render_nav(&[], "", &[], show_all_photos_link(&cfg)).into_string();
        assert!(html.contains("All Photos"));
        assert!(html.contains(r#"href="/all-photos/""#));
    }

    #[test]
    fn all_photos_nav_link_absent_by_default() {
        let cfg = SiteConfig::default();
        let html = render_nav(&[], "", &[], show_all_photos_link(&cfg)).into_string();
        assert!(!html.contains("All Photos"));
    }

    #[test]
    fn all_photos_nav_link_requires_generation() {
        // show_link alone does not produce a link — the target must be generated.
        let mut cfg = SiteConfig::default();
        cfg.full_index.show_link = true;
        cfg.full_index.generates = false;
        assert!(!show_all_photos_link(&cfg));
    }

    #[test]
    fn all_photos_nav_link_marked_current_on_page() {
        let html = render_nav(&[], "all-photos", &[], true).into_string();
        assert!(html.contains(r#"class="current""#));
        assert!(html.contains("All Photos"));
    }

    #[test]
    fn index_page_with_no_albums() {
        let manifest = Manifest {
            navigation: vec![],
            albums: vec![],
            pages: vec![],
            description: None,
            config: SiteConfig::default(),
        };

        let html = render_index(&manifest, "", None, None, &no_snippets(), None).into_string();

        assert!(html.contains("album-grid"));
        assert!(html.contains("Gallery"));
    }

    #[test]
    fn index_page_with_description() {
        let manifest = Manifest {
            navigation: vec![],
            albums: vec![],
            pages: vec![],
            description: Some("<p>Welcome to the gallery.</p>".to_string()),
            config: SiteConfig::default(),
        };

        let html = render_index(&manifest, "", None, None, &no_snippets(), None).into_string();

        assert!(html.contains("has-description"));
        assert!(html.contains("index-header"));
        assert!(html.contains("album-description"));
        assert!(html.contains("Welcome to the gallery."));
        assert!(html.contains("desc-toggle"));
        assert!(html.contains("Read more"));
        assert!(html.contains("Show less"));
        // Should still include the site title in the header
        assert!(html.contains("<h1>Gallery</h1>"));
    }

    #[test]
    fn index_page_no_description_no_header() {
        let manifest = Manifest {
            navigation: vec![],
            albums: vec![],
            pages: vec![],
            description: None,
            config: SiteConfig::default(),
        };

        let html = render_index(&manifest, "", None, None, &no_snippets(), None).into_string();

        assert!(!html.contains("has-description"));
        assert!(!html.contains("index-header"));
        assert!(!html.contains("album-description"));
    }

    // =========================================================================
    // Album page with single image
    // =========================================================================

    #[test]
    fn single_image_album_no_prev_next() {
        let album = Album {
            path: "solo".to_string(),
            title: "Solo Album".to_string(),
            description: None,
            thumbnail: "solo/001-thumb.avif".to_string(),
            images: vec![Image {
                number: 1,
                source_path: "solo/001-photo.jpg".to_string(),
                title: Some("Photo".to_string()),
                description: None,
                dimensions: (1600, 1200),
                generated: {
                    let mut map = BTreeMap::new();
                    map.insert(
                        "800".to_string(),
                        GeneratedVariant {
                            avif: "solo/001-photo-800.avif".to_string(),
                            width: 800,
                            height: 600,
                        },
                    );
                    map
                },
                thumbnail: "solo/001-photo-thumb.avif".to_string(),
                full_index_thumbnail: None,
            }],
            in_nav: true,
            config: SiteConfig::default(),
            support_files: vec![],
        };

        let image = &album.images[0];
        let html = render_image_page(
            &album,
            image,
            None,
            None,
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        // Both prev and next should go back to album
        assert!(html.contains(r#"class="nav-prev" href="../""#));
        assert!(html.contains(r#"class="nav-next" href="../""#));
    }

    #[test]
    fn album_page_no_description() {
        let mut album = create_test_album();
        album.description = None;
        let html = render_album_page(
            &album,
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(!html.contains("album-description"));
        assert!(html.contains("Test Album"));
    }

    #[test]
    fn render_image_page_nav_dots() {
        let album = create_test_album();
        let image = &album.images[0];
        let html = render_image_page(
            &album,
            image,
            None,
            Some(&album.images[1]),
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        // Should contain nav with image-nav class
        assert!(html.contains("image-nav"));
        // Current image dot should have aria-current
        assert!(html.contains(r#"aria-current="true""#));
        // Should have links to both image pages
        assert!(html.contains(r#"href="../1-dawn/""#));
        assert!(html.contains(r#"href="../2/""#));
    }

    #[test]
    fn render_image_page_nav_dots_marks_correct_current() {
        let album = create_test_album();
        // Render second image page
        let html = render_image_page(
            &album,
            &album.images[1],
            Some(&album.images[0]),
            None,
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        // The second dot (href="../2/") should have aria-current
        // The first dot (href="../1-Dawn/") should NOT
        assert!(html.contains(r#"<a href="../2/" aria-current="true">"#));
        assert!(html.contains(r#"<a href="../1-dawn/">"#));
        // Verify the first dot does NOT have aria-current
        assert!(!html.contains(r#"<a href="../1-dawn/" aria-current"#));
    }

    // =========================================================================
    // Custom site_title tests
    // =========================================================================

    #[test]
    fn index_page_uses_custom_site_title() {
        let mut config = SiteConfig::default();
        config.site_title = "My Portfolio".to_string();
        let manifest = Manifest {
            navigation: vec![],
            albums: vec![],
            pages: vec![],
            description: None,
            config,
        };

        let html = render_index(&manifest, "", None, None, &no_snippets(), None).into_string();

        assert!(html.contains("My Portfolio"));
        assert!(!html.contains("Gallery"));
        assert!(html.contains("<title>My Portfolio</title>"));
    }

    #[test]
    fn album_page_breadcrumb_uses_custom_site_title() {
        let album = create_test_album();
        let html = render_album_page(
            &album,
            &[],
            &[],
            "",
            None,
            "My Portfolio",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(html.contains("My Portfolio"));
        assert!(!html.contains("Gallery"));
    }

    #[test]
    fn image_page_breadcrumb_uses_custom_site_title() {
        let album = create_test_album();
        let image = &album.images[0];
        let html = render_image_page(
            &album,
            image,
            None,
            Some(&album.images[1]),
            &[],
            &[],
            "",
            None,
            "My Portfolio",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(html.contains("My Portfolio"));
        assert!(!html.contains("Gallery"));
    }

    #[test]
    fn content_page_breadcrumb_uses_custom_site_title() {
        let page = Page {
            title: "About".to_string(),
            link_title: "About".to_string(),
            slug: "about".to_string(),
            body: "# About\n\nContent.".to_string(),
            in_nav: true,
            sort_key: 40,
            is_link: false,
        };
        let html = render_page(
            &page,
            &[],
            &[],
            "",
            None,
            "My Portfolio",
            None,
            &no_snippets(),
            false,
        )
        .into_string();

        assert!(html.contains("My Portfolio"));
        assert!(!html.contains("Gallery"));
    }

    #[test]
    fn pwa_assets_present() {
        let manifest = Manifest {
            navigation: vec![],
            albums: vec![],
            pages: vec![],
            description: None,
            config: SiteConfig::default(),
        };

        let html = render_index(&manifest, "", None, None, &no_snippets(), None).into_string();

        assert!(html.contains(r#"<link rel="manifest" href="/site.webmanifest">"#));
        assert!(html.contains(r#"<link rel="apple-touch-icon" href="/apple-touch-icon.png">"#));
        assert!(html.contains("navigator.serviceWorker.register('/sw.js');"));
        assert!(html.contains("beforeinstallprompt"));
    }

    // =========================================================================
    // Custom snippets tests
    // =========================================================================

    #[test]
    fn no_custom_css_link_by_default() {
        let content = html! { p { "test" } };
        let doc = base_document(
            "Test",
            "",
            None,
            None,
            None,
            None,
            &no_snippets(),
            None,
            content,
        )
        .into_string();
        assert!(!doc.contains("custom.css"));
    }

    #[test]
    fn custom_css_link_injected_when_present() {
        let snippets = CustomSnippets {
            has_custom_css: true,
            ..Default::default()
        };
        let content = html! { p { "test" } };
        let doc = base_document("Test", "", None, None, None, None, &snippets, None, content)
            .into_string();
        assert!(doc.contains(r#"<link rel="stylesheet" href="/custom.css">"#));
    }

    #[test]
    fn custom_css_link_after_main_style() {
        let snippets = CustomSnippets {
            has_custom_css: true,
            ..Default::default()
        };
        let content = html! { p { "test" } };
        let doc = base_document(
            "Test", "body{}", None, None, None, None, &snippets, None, content,
        )
        .into_string();
        let style_pos = doc.find("</style>").unwrap();
        let link_pos = doc.find(r#"href="/custom.css""#).unwrap();
        assert!(
            link_pos > style_pos,
            "custom.css link should appear after main <style>"
        );
    }

    #[test]
    fn head_html_injected_when_present() {
        let snippets = CustomSnippets {
            head_html: Some(r#"<script>console.log("analytics")</script>"#.to_string()),
            ..Default::default()
        };
        let content = html! { p { "test" } };
        let doc = base_document("Test", "", None, None, None, None, &snippets, None, content)
            .into_string();
        assert!(doc.contains(r#"<script>console.log("analytics")</script>"#));
    }

    #[test]
    fn head_html_inside_head_element() {
        let snippets = CustomSnippets {
            head_html: Some("<!-- custom head -->".to_string()),
            ..Default::default()
        };
        let content = html! { p { "test" } };
        let doc = base_document("Test", "", None, None, None, None, &snippets, None, content)
            .into_string();
        let head_end = doc.find("</head>").unwrap();
        let snippet_pos = doc.find("<!-- custom head -->").unwrap();
        assert!(
            snippet_pos < head_end,
            "head.html should appear inside <head>"
        );
    }

    #[test]
    fn no_head_html_by_default() {
        let content = html! { p { "test" } };
        let doc = base_document(
            "Test",
            "",
            None,
            None,
            None,
            None,
            &no_snippets(),
            None,
            content,
        )
        .into_string();
        // Only the standard head content should be present
        assert!(!doc.contains("<!-- custom"));
    }

    #[test]
    fn body_end_html_injected_when_present() {
        let snippets = CustomSnippets {
            body_end_html: Some(r#"<script src="/tracking.js"></script>"#.to_string()),
            ..Default::default()
        };
        let content = html! { p { "test" } };
        let doc = base_document("Test", "", None, None, None, None, &snippets, None, content)
            .into_string();
        assert!(doc.contains(r#"<script src="/tracking.js"></script>"#));
    }

    #[test]
    fn body_end_html_inside_body_before_close() {
        let snippets = CustomSnippets {
            body_end_html: Some("<!-- body end -->".to_string()),
            ..Default::default()
        };
        let content = html! { p { "test" } };
        let doc = base_document("Test", "", None, None, None, None, &snippets, None, content)
            .into_string();
        let body_end = doc.find("</body>").unwrap();
        let snippet_pos = doc.find("<!-- body end -->").unwrap();
        assert!(
            snippet_pos < body_end,
            "body-end.html should appear before </body>"
        );
    }

    #[test]
    fn body_end_html_after_content() {
        let snippets = CustomSnippets {
            body_end_html: Some("<!-- body end -->".to_string()),
            ..Default::default()
        };
        let content = html! { p { "main content" } };
        let doc = base_document("Test", "", None, None, None, None, &snippets, None, content)
            .into_string();
        let content_pos = doc.find("main content").unwrap();
        let snippet_pos = doc.find("<!-- body end -->").unwrap();
        assert!(
            snippet_pos > content_pos,
            "body-end.html should appear after main content"
        );
    }

    #[test]
    fn all_snippets_injected_together() {
        let snippets = CustomSnippets {
            has_custom_css: true,
            head_html: Some("<!-- head snippet -->".to_string()),
            body_end_html: Some("<!-- body snippet -->".to_string()),
        };
        let content = html! { p { "test" } };
        let doc = base_document("Test", "", None, None, None, None, &snippets, None, content)
            .into_string();
        assert!(doc.contains(r#"href="/custom.css""#));
        assert!(doc.contains("<!-- head snippet -->"));
        assert!(doc.contains("<!-- body snippet -->"));
    }

    #[test]
    fn snippets_appear_in_all_page_types() {
        let snippets = CustomSnippets {
            has_custom_css: true,
            head_html: Some("<!-- head -->".to_string()),
            body_end_html: Some("<!-- body -->".to_string()),
        };

        // Index page
        let manifest = Manifest {
            navigation: vec![],
            albums: vec![],
            pages: vec![],
            description: None,
            config: SiteConfig::default(),
        };
        let html = render_index(&manifest, "", None, None, &snippets, None).into_string();
        assert!(html.contains("custom.css"));
        assert!(html.contains("<!-- head -->"));
        assert!(html.contains("<!-- body -->"));

        // Album page
        let album = create_test_album();
        let html = render_album_page(
            &album,
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &snippets,
            false,
            None,
        )
        .into_string();
        assert!(html.contains("custom.css"));
        assert!(html.contains("<!-- head -->"));
        assert!(html.contains("<!-- body -->"));

        // Content page
        let page = make_page("about", "About", true, false);
        let html =
            render_page(&page, &[], &[], "", None, "Gallery", None, &snippets, false).into_string();
        assert!(html.contains("custom.css"));
        assert!(html.contains("<!-- head -->"));
        assert!(html.contains("<!-- body -->"));

        // Image page
        let html = render_image_page(
            &album,
            &album.images[0],
            None,
            Some(&album.images[1]),
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &snippets,
            false,
            None,
        )
        .into_string();
        assert!(html.contains("custom.css"));
        assert!(html.contains("<!-- head -->"));
        assert!(html.contains("<!-- body -->"));
    }

    #[test]
    fn detect_custom_snippets_finds_files() {
        let tmp = tempfile::TempDir::new().unwrap();

        // No files → empty snippets
        let snippets = detect_custom_snippets(tmp.path());
        assert!(!snippets.has_custom_css);
        assert!(snippets.head_html.is_none());
        assert!(snippets.body_end_html.is_none());

        // Create custom.css
        fs::write(tmp.path().join("custom.css"), "body { color: red; }").unwrap();
        let snippets = detect_custom_snippets(tmp.path());
        assert!(snippets.has_custom_css);
        assert!(snippets.head_html.is_none());

        // Create head.html
        fs::write(tmp.path().join("head.html"), "<meta name=\"test\">").unwrap();
        let snippets = detect_custom_snippets(tmp.path());
        assert!(snippets.has_custom_css);
        assert_eq!(snippets.head_html.as_deref(), Some("<meta name=\"test\">"));

        // Create body-end.html
        fs::write(
            tmp.path().join("body-end.html"),
            "<script>alert(1)</script>",
        )
        .unwrap();
        let snippets = detect_custom_snippets(tmp.path());
        assert!(snippets.has_custom_css);
        assert!(snippets.head_html.is_some());
        assert_eq!(
            snippets.body_end_html.as_deref(),
            Some("<script>alert(1)</script>")
        );
    }

    // =========================================================================
    // image_sizes_attr tests
    // =========================================================================

    #[test]
    fn sizes_attr_landscape_uses_vw() {
        // 1600x1200 → aspect 1.333, 90*1.333 = 120 > 100 → landscape branch
        let attr = image_sizes_attr(1600.0 / 1200.0, 1400);
        assert!(
            attr.contains("95vw"),
            "desktop should use 95vw for landscape: {attr}"
        );
        assert!(
            attr.contains("1400px"),
            "should cap at max generated width: {attr}"
        );
    }

    #[test]
    fn sizes_attr_portrait_uses_vh() {
        // 1200x1600 → aspect 0.75, 90*0.75 = 67.5 < 100 → portrait branch
        let attr = image_sizes_attr(1200.0 / 1600.0, 600);
        assert!(
            attr.contains("vh"),
            "desktop should use vh for portrait: {attr}"
        );
        assert!(
            attr.contains("67.5vh"),
            "should be 90 * 0.75 = 67.5vh: {attr}"
        );
        assert!(
            attr.contains("600px"),
            "should cap at max generated width: {attr}"
        );
    }

    #[test]
    fn sizes_attr_square_uses_vh() {
        // 1:1 → aspect 1.0, 90*1.0 = 90 < 100 → portrait/square branch
        let attr = image_sizes_attr(1.0, 2080);
        assert!(
            attr.contains("vh"),
            "square treated as height-constrained: {attr}"
        );
        assert!(attr.contains("90.0vh"), "should be 90 * 1.0: {attr}");
    }

    #[test]
    fn sizes_attr_mobile_always_100vw() {
        for aspect in [0.5, 0.75, 1.0, 1.333, 2.0] {
            let attr = image_sizes_attr(aspect, 1400);
            assert!(
                attr.contains("(max-width: 800px) min(100vw,"),
                "mobile should always be 100vw: {attr}"
            );
        }
    }

    #[test]
    fn sizes_attr_caps_at_max_width() {
        let attr = image_sizes_attr(1.5, 900);
        // Both mobile and desktop min() should reference the 900px cap
        assert_eq!(
            attr.matches("900px").count(),
            2,
            "should have px cap in both conditions: {attr}"
        );
    }

    // =========================================================================
    // srcset w-descriptor correctness
    // =========================================================================

    #[test]
    fn srcset_uses_actual_width_not_target_for_portrait() {
        let album = create_test_album();
        let image = &album.images[1]; // portrait 1200x1600, generated width=600
        let nav = vec![];
        let html = render_image_page(
            &album,
            image,
            Some(&album.images[0]),
            None,
            &nav,
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        // Portrait: target key is "800" (longer edge=height) but actual width is 600
        assert!(
            html.contains("600w"),
            "srcset should use actual width 600, not target 800: {html}"
        );
        assert!(
            !html.contains("800w"),
            "srcset must not use target (height) as w descriptor"
        );
    }

    #[test]
    fn srcset_uses_actual_width_for_landscape() {
        let album = create_test_album();
        let image = &album.images[0]; // landscape 1600x1200, generated widths 800 and 1400
        let nav = vec![];
        let html = render_image_page(
            &album,
            image,
            None,
            Some(&album.images[1]),
            &nav,
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(html.contains("800w"));
        assert!(html.contains("1400w"));
    }

    // ========================================================================
    // Open Graph metadata
    // ========================================================================

    #[test]
    fn pick_og_variant_picks_smallest_above_target() {
        let album = create_test_album();
        // image[0] has variants 800 and 1400; target is 1200, so 1400 wins.
        let picked = pick_og_variant(&album.images[0]).expect("variant exists");
        assert_eq!(picked.width, 1400);
    }

    #[test]
    fn album_cover_image_matches_designated_thumbnail() {
        // Simulate the v0.9 "thumb-designated" flow: album.thumbnail points
        // at the second image's thumbnail, not the first. OG must pick the
        // image whose thumbnail the gallery grid is actually showing.
        let mut album = create_test_album();
        album.thumbnail = album.images[1].thumbnail.clone();
        let cover = album_cover_image(&album).expect("cover exists");
        assert_eq!(cover.number, album.images[1].number);
    }

    #[test]
    fn album_cover_image_falls_back_to_first_when_paths_disagree() {
        // Defensive fallback: if album.thumbnail doesn't match any image's
        // thumbnail (shouldn't happen in current pipeline but we don't want
        // OG to silently become None), return the first image.
        let mut album = create_test_album();
        album.thumbnail = "unrelated/path.avif".to_string();
        let cover = album_cover_image(&album).expect("cover exists");
        assert_eq!(cover.number, album.images[0].number);
    }

    #[test]
    fn pick_og_variant_falls_back_to_largest_when_all_below_target() {
        // image[1] has a single variant at target_size 800 but is portrait-
        // oriented (1200x1600 source), so the variant's actual width is 600
        // (scaled to fit the 800px long-edge target). Since that's below the
        // 1200 target, fallback = largest available = width 600.
        let album = create_test_album();
        let picked = pick_og_variant(&album.images[1]).expect("variant exists");
        assert_eq!(picked.width, 600);
    }

    #[test]
    fn absolute_url_joins_base_and_path() {
        assert_eq!(
            absolute_url("https://example.com", "NY/Night/001.avif"),
            "https://example.com/NY/Night/001.avif"
        );
    }

    #[test]
    fn absolute_url_tolerates_trailing_and_leading_slashes() {
        assert_eq!(
            absolute_url("https://example.com/", "/NY/001.avif"),
            "https://example.com/NY/001.avif"
        );
    }

    #[test]
    fn absolute_url_empty_path_is_site_root() {
        assert_eq!(
            absolute_url("https://example.com", ""),
            "https://example.com/"
        );
    }

    #[test]
    fn og_description_joins_site_title_segments_and_trailing() {
        let segments = vec![("NY", "NY"), ("Night", "NY/Night")];
        let out = og_description("Gallery", &segments, &["City"]);
        assert_eq!(out, "Gallery › NY › Night › City");
    }

    fn ny_navigation() -> Vec<NavItem> {
        vec![NavItem {
            title: "NY".to_string(),
            path: "NY".to_string(),
            source_dir: String::new(),
            description: None,
            children: vec![NavItem {
                title: "Night".to_string(),
                path: "NY/Night".to_string(),
                source_dir: String::new(),
                description: None,
                children: vec![],
            }],
        }]
    }

    #[test]
    fn build_og_for_nested_album_yields_absolute_paths_and_breadcrumb() {
        let album = create_nested_test_album();
        let navigation = ny_navigation();
        let og = build_og_for_album("https://example.com", &album, &navigation, "Gallery").unwrap();

        assert_eq!(og.title, "Night");
        assert_eq!(og.page_url, "https://example.com/NY/Night/");
        assert_eq!(
            og.image_url,
            "https://example.com/NY/Night/001-city-1400.avif"
        );
        assert_eq!(og.description, "Gallery › NY › Night");
        assert_eq!(og.site_name, "Gallery");
    }

    #[test]
    fn build_og_for_image_includes_image_label_in_description() {
        let album = create_nested_test_album();
        let image = &album.images[0];
        let navigation = ny_navigation();
        let og = build_og_for_image(
            "https://example.com",
            &album,
            image,
            0,
            &navigation,
            "Gallery",
        )
        .unwrap();
        // Single-image album → 1-wide index, so label is "1. City".
        assert_eq!(og.description, "Gallery › NY › Night › 1. City");
        assert!(og.page_url.starts_with("https://example.com/NY/Night/"));
        assert!(og.page_url.ends_with("-city/"));
        assert_eq!(
            og.image_url,
            "https://example.com/NY/Night/001-city-1400.avif"
        );
    }

    #[test]
    fn render_album_page_emits_og_tags_when_og_is_some() {
        let album = create_test_album();
        let og = build_og_for_album("https://example.com", &album, &[], "Gallery")
            .expect("album has images");
        let html = render_album_page(
            &album,
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            Some(&og),
        )
        .into_string();

        assert!(html.contains(r#"<meta property="og:title" content="Test Album">"#));
        assert!(html.contains(r#"<meta property="og:type" content="website">"#));
        assert!(html.contains(r#"<meta property="og:site_name" content="Gallery">"#));
        assert!(html.contains(r#"content="https://example.com/test/""#));
        assert!(html.contains(r#"content="https://example.com/test/001-dawn-1400.avif""#));
        assert!(html.contains(r#"<meta name="twitter:card" content="summary_large_image">"#));
    }

    #[test]
    fn render_album_page_emits_no_og_tags_when_og_is_none() {
        let album = create_test_album();
        let html = render_album_page(
            &album,
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            None,
        )
        .into_string();

        assert!(!html.contains("og:title"));
        assert!(!html.contains("og:image"));
        assert!(!html.contains("twitter:card"));
    }

    #[test]
    fn render_image_page_og_description_is_full_breadcrumb() {
        let album = create_test_album();
        let image = &album.images[0];
        let og = build_og_for_image("https://example.com", &album, image, 0, &[], "Gallery")
            .expect("image has variants");
        let html = render_image_page(
            &album,
            image,
            None,
            Some(&album.images[1]),
            &[],
            &[],
            "",
            None,
            "Gallery",
            None,
            &no_snippets(),
            false,
            Some(&og),
        )
        .into_string();

        // Breadcrumb description echoes site_title › album_title › image_label
        // (no nav segments because the test album has no navigation tree).
        assert!(html.contains(r#"content="Gallery › Test Album › 1. Dawn""#));
    }
}
