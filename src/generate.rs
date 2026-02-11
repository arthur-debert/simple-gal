//! HTML site generation.
//!
//! Stage 3 of the Simple Gal build pipeline. Takes the processed manifest and
//! generates the final static HTML site.
//!
//! ## Generated Pages
//!
//! - **Index page** (`/index.html`): Album grid showing thumbnails of all albums
//! - **Album pages** (`/{album}/index.html`): Thumbnail grid for an album
//! - **Image pages** (`/{album}/{n}-{slug}.html`): Full-screen image viewer with navigation
//! - **Content pages** (`/{slug}.html`): Markdown pages (e.g. about, contact)
//!
//! ## Features
//!
//! - **Responsive images**: Uses `<picture>` with AVIF and WebP srcsets
//! - **Collapsible navigation**: Details/summary for mobile-friendly nav
//! - **Keyboard navigation**: Arrow keys and swipe gestures for image browsing
//! - **View transitions**: Smooth page-to-page animations (where supported)
//! - **Configurable colors**: CSS custom properties generated from config.toml
//!
//! ## Output Structure
//!
//! ```text
//! dist/
//! ├── index.html                 # Home/gallery page
//! ├── about.html                 # Content page (from 040-about.md)
//! ├── Landscapes/
//! │   ├── index.html             # Album page
//! │   ├── 1-dawn.html            # Image viewer pages
//! │   ├── 2-sunset.html
//! │   ├── 001-dawn-800.avif      # Processed images (copied)
//! │   ├── 001-dawn-800.webp
//! │   └── ...
//! └── Travel/
//!     └── ...
//! ```
//!
//! ## CSS and JavaScript
//!
//! Static assets are embedded at compile time:
//! - `static/style.css`: Base styles (colors injected from config)
//! - `static/nav.js`: Keyboard and touch navigation
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
}

#[derive(Debug, Deserialize)]
pub struct GeneratedVariant {
    pub avif: String,
    pub webp: String,
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

const IMAGE_SIZES: &str = "(max-width: 800px) 100vw, 80vw";

/// Zero-padding width for image indices, based on album size.
fn index_width(total: usize) -> usize {
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
fn image_page_url(position: usize, total: usize, title: Option<&str>) -> String {
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
/// Replaces spaces and dots with hyphens and collapses consecutive hyphens.
fn escape_for_url(title: &str) -> String {
    let mut result = String::with_capacity(title.len());
    let mut prev_dash = false;
    for c in title.chars() {
        if c == ' ' || c == '.' {
            if !prev_dash {
                result.push('-');
            }
            prev_dash = true;
        } else {
            result.push(c);
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
    //      Produces :root { --color-*, --frame-*, --font-*, … }
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

    // Write PWA assets (default implementation)
    // We write these *before* copying assets so user can override them

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
        println!("Copied static assets from {}", assets_path.display());
    }

    // Copy processed images to output
    copy_dir_recursive(processed_dir, output_dir)?;

    // Detect favicon in output directory for <link rel="icon"> injection
    let favicon_href = detect_favicon(output_dir);

    // Generate index page
    let index_html = render_index(
        &manifest,
        &css,
        font_url.as_deref(),
        favicon_href.as_deref(),
    );
    fs::write(output_dir.join("index.html"), index_html.into_string())?;
    println!("Generated index.html");

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
        );
        let filename = format!("{}.html", page.slug);
        fs::write(output_dir.join(&filename), page_html.into_string())?;
        println!("Generated {}", filename);
    }

    // Generate album pages
    for album in &manifest.albums {
        let album_dir = output_dir.join(&album.path);
        fs::create_dir_all(&album_dir)?;

        let album_html = render_album_page(
            album,
            &manifest.navigation,
            &manifest.pages,
            &css,
            font_url.as_deref(),
            &manifest.config.site_title,
            favicon_href.as_deref(),
        );
        fs::write(album_dir.join("index.html"), album_html.into_string())?;
        println!("Generated {}/index.html", album.path);

        // Generate image pages
        for (idx, image) in album.images.iter().enumerate() {
            let prev = if idx > 0 {
                Some(&album.images[idx - 1])
            } else {
                None
            };
            let next = album.images.get(idx + 1);

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
            );
            let image_dir_name =
                image_page_url(idx + 1, album.images.len(), image.title.as_deref());
            let image_dir = album_dir.join(&image_dir_name);
            fs::create_dir_all(&image_dir)?;
            fs::write(image_dir.join("index.html"), image_html.into_string())?;
        }
        println!(
            "Generated {} image pages for {}",
            album.images.len(),
            album.title
        );
    }

    println!("Site generated at {}", output_dir.display());
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
fn base_document(
    title: &str,
    css: &str,
    font_url: Option<&str>,
    body_class: Option<&str>,
    head_extra: Option<Markup>,
    favicon_href: Option<&str>,
    content: Markup,
) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { (title) }
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
                @if let Some(extra) = head_extra {
                    (extra)
                }
                script {
                    (PreEscaped(r#"
                        if ('serviceWorker' in navigator) {
                            window.addEventListener('load', () => {
                                navigator.serviceWorker.register('/sw.js');
                            });
                        }
                    "#))
                }
            }
            body class=[body_class] {
                (content)
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
pub fn render_nav(items: &[NavItem], current_path: &str, pages: &[Page]) -> Markup {
    let nav_pages: Vec<&Page> = pages.iter().filter(|p| p.in_nav).collect();

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
                span.nav-group { (item.title) }
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

/// Renders the index/home page with album grid
fn render_index(
    manifest: &Manifest,
    css: &str,
    font_url: Option<&str>,
    favicon_href: Option<&str>,
) -> Markup {
    let nav = render_nav(&manifest.navigation, "", &manifest.pages);

    let breadcrumb = html! {
        a href="/" { (&manifest.config.site_title) }
    };

    let content = html! {
        (site_header(breadcrumb, nav))
        main.index-page {
            div.album-grid {
                @for album in manifest.albums.iter().filter(|a| a.in_nav) {
                    a.album-card href={ (album.path) "/" } {
                        img src=(album.thumbnail) alt=(album.title) loading="lazy";
                        span.album-title { (album.title) }
                    }
                }
            }
        }
    };

    base_document(
        &manifest.config.site_title,
        css,
        font_url,
        None,
        None,
        favicon_href,
        content,
    )
}

/// Renders an album page with thumbnail grid
fn render_album_page(
    album: &Album,
    navigation: &[NavItem],
    pages: &[Page],
    css: &str,
    font_url: Option<&str>,
    site_title: &str,
    favicon_href: Option<&str>,
) -> Markup {
    let nav = render_nav(navigation, &album.path, pages);

    let breadcrumb = html! {
        a href="/" { (site_title) }
        " › "
        (album.title)
    };

    // Strip album path prefix since album page is inside the album directory
    let strip_prefix = |path: &str| -> String {
        path.strip_prefix(&album.path)
            .and_then(|p| p.strip_prefix('/'))
            .unwrap_or(path)
            .to_string()
    };

    let content = html! {
        (site_header(breadcrumb, nav))
        main.album-page {
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
) -> Markup {
    let nav = render_nav(navigation, &album.path, pages);

    // Strip album path prefix and add ../ since image pages are in subdirectories
    let strip_prefix = |path: &str| -> String {
        let relative = path
            .strip_prefix(&album.path)
            .and_then(|p| p.strip_prefix('/'))
            .unwrap_or(path);
        format!("../{}", relative)
    };

    // Build srcset for a given image's avif variants
    let avif_srcset_for = |img: &Image| -> String {
        img.generated
            .iter()
            .map(|(size, variant)| format!("{} {}w", strip_prefix(&variant.avif), size))
            .collect::<Vec<_>>()
            .join(", ")
    };

    // Build srcsets
    let sizes: Vec<_> = image.generated.iter().collect();

    let srcset_avif: String = avif_srcset_for(image);

    let srcset_webp: String = sizes
        .iter()
        .map(|(size, variant)| format!("{} {}w", strip_prefix(&variant.webp), size))
        .collect::<Vec<_>>()
        .join(", ");

    // Use middle size as default
    let default_src = sizes
        .get(sizes.len() / 2)
        .map(|(_, v)| strip_prefix(&v.webp))
        .unwrap_or_default();

    // Pick a single middle-size AVIF URL for adjacent image prefetch
    let mid_avif = |img: &Image| -> String {
        let sizes: Vec<_> = img.generated.iter().collect();
        sizes
            .get(sizes.len() / 2)
            .map(|(_, v)| strip_prefix(&v.avif))
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

    let breadcrumb = html! {
        a href="/" { (site_title) }
        " › "
        a href="../" { (album.title) }
        " › "
        (image_label)
    };

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
                    picture {
                        source type="image/avif" srcset=(srcset_avif) sizes=(IMAGE_SIZES);
                        source type="image/webp" srcset=(srcset_webp) sizes=(IMAGE_SIZES);
                        img #main-image src=(default_src) alt=(alt_text);
                    }
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
        }
        a.nav-prev href=(prev_url) aria-label="Previous image" {}
        a.nav-next href=(next_url) aria-label="Next image" {}
        script { (PreEscaped(JS)) }
    };

    base_document(
        &page_title,
        css,
        font_url,
        Some(body_class),
        Some(head_extra),
        favicon_href,
        content,
    )
}

/// Renders a content page from markdown
fn render_page(
    page: &Page,
    navigation: &[NavItem],
    pages: &[Page],
    css: &str,
    font_url: Option<&str>,
    site_title: &str,
    favicon_href: Option<&str>,
) -> Markup {
    let nav = render_nav(navigation, &page.slug, pages);

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
        content,
    )
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
            children: vec![],
        }];
        let html = render_nav(&items, "", &[]).into_string();
        assert!(html.contains("Album One"));
        assert!(html.contains("/010-one/"));
    }

    #[test]
    fn nav_includes_pages() {
        let pages = vec![make_page("about", "About", true, false)];
        let html = render_nav(&[], "", &pages).into_string();
        assert!(html.contains("About"));
        assert!(html.contains("/about.html"));
    }

    #[test]
    fn nav_hides_unnumbered_pages() {
        let pages = vec![make_page("notes", "Notes", false, false)];
        let html = render_nav(&[], "", &pages).into_string();
        assert!(!html.contains("Notes"));
        // No separator either when no nav pages
        assert!(!html.contains("nav-separator"));
    }

    #[test]
    fn nav_renders_link_page_as_external() {
        let pages = vec![make_page("github", "GitHub", true, true)];
        let html = render_nav(&[], "", &pages).into_string();
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
                children: vec![],
            },
            NavItem {
                title: "Second".to_string(),
                path: "020-second".to_string(),
                children: vec![],
            },
        ];
        let html = render_nav(&items, "020-second", &[]).into_string();
        // The second item should have the current class
        assert!(html.contains(r#"class="current"#));
    }

    #[test]
    fn nav_marks_current_page() {
        let pages = vec![make_page("about", "About", true, false)];
        let html = render_nav(&[], "about", &pages).into_string();
        assert!(html.contains(r#"class="current"#));
    }

    #[test]
    fn nav_renders_nested_children() {
        let items = vec![NavItem {
            title: "Parent".to_string(),
            path: "010-parent".to_string(),
            children: vec![NavItem {
                title: "Child".to_string(),
                path: "010-parent/010-child".to_string(),
                children: vec![],
            }],
        }];
        let html = render_nav(&items, "", &[]).into_string();
        assert!(html.contains("Parent"));
        assert!(html.contains("Child"));
        assert!(html.contains("nav-group")); // Parent should have nav-group class
    }

    #[test]
    fn nav_separator_only_when_pages() {
        // No pages = no separator
        let html_no_pages = render_nav(&[], "", &[]).into_string();
        assert!(!html_no_pages.contains("nav-separator"));

        // With nav pages = separator
        let pages = vec![make_page("about", "About", true, false)];
        let html_with_pages = render_nav(&[], "", &pages).into_string();
        assert!(html_with_pages.contains("nav-separator"));
    }

    #[test]
    fn base_document_includes_doctype() {
        let content = html! { p { "test" } };
        let doc = base_document("Test", "body {}", None, None, None, None, content).into_string();
        assert!(doc.starts_with("<!DOCTYPE html>"));
    }

    #[test]
    fn base_document_applies_body_class() {
        let content = html! { p { "test" } };
        let doc =
            base_document("Test", "", None, Some("image-view"), None, None, content).into_string();
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
            thumbnail: "test/001-image-thumb.webp".to_string(),
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
                                webp: "test/001-dawn-800.webp".to_string(),
                                width: 800,
                                height: 600,
                            },
                        );
                        map.insert(
                            "1400".to_string(),
                            GeneratedVariant {
                                avif: "test/001-dawn-1400.avif".to_string(),
                                webp: "test/001-dawn-1400.webp".to_string(),
                                width: 1400,
                                height: 1050,
                            },
                        );
                        map
                    },
                    thumbnail: "test/001-dawn-thumb.webp".to_string(),
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
                                webp: "test/002-night-800.webp".to_string(),
                                width: 600,
                                height: 800,
                            },
                        );
                        map
                    },
                    thumbnail: "test/002-night-thumb.webp".to_string(),
                },
            ],
            in_nav: true,
            config: SiteConfig::default(),
        }
    }

    #[test]
    fn render_album_page_includes_title() {
        let album = create_test_album();
        let nav = vec![];
        let html = render_album_page(&album, &nav, &[], "", None, "Gallery", None).into_string();

        assert!(html.contains("Test Album"));
        assert!(html.contains("<h1>"));
    }

    #[test]
    fn render_album_page_includes_description() {
        let album = create_test_album();
        let nav = vec![];
        let html = render_album_page(&album, &nav, &[], "", None, "Gallery", None).into_string();

        assert!(html.contains("A test album description"));
        assert!(html.contains("album-description"));
    }

    #[test]
    fn render_album_page_thumbnail_links() {
        let album = create_test_album();
        let nav = vec![];
        let html = render_album_page(&album, &nav, &[], "", None, "Gallery", None).into_string();

        // Should have links to image pages (1-Dawn/, 2/)
        assert!(html.contains("1-Dawn/"));
        assert!(html.contains("2/"));
        // Thumbnails should have paths relative to album dir
        assert!(html.contains("001-dawn-thumb.webp"));
    }

    #[test]
    fn render_album_page_breadcrumb() {
        let album = create_test_album();
        let nav = vec![];
        let html = render_album_page(&album, &nav, &[], "", None, "Gallery", None).into_string();

        // Breadcrumb should link to gallery root
        assert!(html.contains(r#"href="/""#));
        assert!(html.contains("Gallery"));
    }

    #[test]
    fn render_image_page_includes_picture_element() {
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
        )
        .into_string();

        assert!(html.contains("<picture>"));
        assert!(html.contains("image/avif"));
        assert!(html.contains("image/webp"));
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
        )
        .into_string();
        assert!(html2.contains(r#"class="nav-prev" href="../1-Dawn/""#));
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
        let html = render_page(&page, &[], &[], "", None, "Gallery", None).into_string();

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
        let html = render_page(&page, &[], &[], "", None, "Gallery", None).into_string();

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
            children: vec![],
        }];
        let html = render_nav(&items, "", &[]).into_string();

        // Should be escaped, not raw script tag
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;script&gt;"));
    }

    // =========================================================================
    // escape_for_url tests
    // =========================================================================

    #[test]
    fn escape_for_url_spaces_become_dashes() {
        assert_eq!(escape_for_url("My Title"), "My-Title");
    }

    #[test]
    fn escape_for_url_dots_become_dashes() {
        assert_eq!(escape_for_url("St. Louis"), "St-Louis");
    }

    #[test]
    fn escape_for_url_collapses_consecutive() {
        assert_eq!(escape_for_url("A.  B"), "A-B");
    }

    #[test]
    fn escape_for_url_strips_leading_trailing() {
        assert_eq!(escape_for_url(". Title ."), "Title");
    }

    #[test]
    fn escape_for_url_preserves_dashes() {
        assert_eq!(escape_for_url("My-Title"), "My-Title");
    }

    #[test]
    fn image_page_url_with_title() {
        assert_eq!(image_page_url(3, 15, Some("Dawn")), "03-Dawn/");
    }

    #[test]
    fn image_page_url_without_title() {
        assert_eq!(image_page_url(3, 15, None), "03/");
    }

    #[test]
    fn image_page_url_title_with_spaces() {
        assert_eq!(image_page_url(1, 5, Some("My Museum")), "1-My-Museum/");
    }

    #[test]
    fn image_page_url_title_with_dot() {
        assert_eq!(image_page_url(1, 5, Some("St. Louis")), "1-St-Louis/");
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
        )
        .into_string();

        // Should have a prefetch link with the prev image's middle-size avif
        assert!(html.contains(r#"rel="prefetch""#));
        assert!(html.contains("001-dawn-800.avif"));
        // Single URL (href), not a srcset — should not contain both sizes
        assert!(!html.contains("001-dawn-1400.avif"));
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
        let html = render_album_page(&album, &[], &[], &css, None, "Gallery", None).into_string();

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
        config.theme.frame_x.size = "5vw".to_string();

        let theme_css = crate::config::generate_theme_css(&config.theme);
        let album = create_test_album();
        let html =
            render_album_page(&album, &[], &[], &theme_css, None, "Gallery", None).into_string();

        assert!(html.contains("--thumbnail-gap: 0.5rem"));
        assert!(html.contains("--frame-width-x: clamp(1rem, 5vw, 2.5rem)"));
        assert!(html.contains("--frame-width-y:"));
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
            navigation: vec![],
            albums: vec![
                Album {
                    path: "visible".to_string(),
                    title: "Visible".to_string(),
                    description: None,
                    thumbnail: "visible/thumb.webp".to_string(),
                    images: vec![],
                    in_nav: true,
                    config: SiteConfig::default(),
                },
                Album {
                    path: "hidden".to_string(),
                    title: "Hidden".to_string(),
                    description: None,
                    thumbnail: "hidden/thumb.webp".to_string(),
                    images: vec![],
                    in_nav: false,
                    config: SiteConfig::default(),
                },
            ],
            pages: vec![],
            config: SiteConfig::default(),
        };

        let html = render_index(&manifest, "", None, None).into_string();

        assert!(html.contains("Visible"));
        assert!(!html.contains("Hidden"));
    }

    #[test]
    fn index_page_with_no_albums() {
        let manifest = Manifest {
            navigation: vec![],
            albums: vec![],
            pages: vec![],
            config: SiteConfig::default(),
        };

        let html = render_index(&manifest, "", None, None).into_string();

        assert!(html.contains("album-grid"));
        assert!(html.contains("Gallery"));
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
            thumbnail: "solo/001-thumb.webp".to_string(),
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
                            webp: "solo/001-photo-800.webp".to_string(),
                            width: 800,
                            height: 600,
                        },
                    );
                    map
                },
                thumbnail: "solo/001-photo-thumb.webp".to_string(),
            }],
            in_nav: true,
            config: SiteConfig::default(),
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
        let html = render_album_page(&album, &[], &[], "", None, "Gallery", None).into_string();

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
        )
        .into_string();

        // Should contain nav with image-nav class
        assert!(html.contains("image-nav"));
        // Current image dot should have aria-current
        assert!(html.contains(r#"aria-current="true""#));
        // Should have links to both image pages
        assert!(html.contains(r#"href="../1-Dawn/""#));
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
        )
        .into_string();

        // The second dot (href="../2/") should have aria-current
        // The first dot (href="../1-Dawn/") should NOT
        assert!(html.contains(r#"<a href="../2/" aria-current="true">"#));
        assert!(html.contains(r#"<a href="../1-Dawn/">"#));
        // Verify the first dot does NOT have aria-current
        assert!(!html.contains(r#"<a href="../1-Dawn/" aria-current"#));
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
            config,
        };

        let html = render_index(&manifest, "", None, None).into_string();

        assert!(html.contains("My Portfolio"));
        assert!(!html.contains("Gallery"));
        assert!(html.contains("<title>My Portfolio</title>"));
    }

    #[test]
    fn album_page_breadcrumb_uses_custom_site_title() {
        let album = create_test_album();
        let html =
            render_album_page(&album, &[], &[], "", None, "My Portfolio", None).into_string();

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
        let html = render_page(&page, &[], &[], "", None, "My Portfolio", None).into_string();

        assert!(html.contains("My Portfolio"));
        assert!(!html.contains("Gallery"));
    }

    #[test]
    fn pwa_assets_present() {
        let manifest = Manifest {
            navigation: vec![],
            albums: vec![],
            pages: vec![],
            config: SiteConfig::default(),
        };

        let html = render_index(&manifest, "", None, None).into_string();

        assert!(html.contains(r#"<link rel="manifest" href="/site.webmanifest">"#));
        assert!(html.contains(r#"<link rel="apple-touch-icon" href="/apple-touch-icon.png">"#));
        assert!(html.contains("navigator.serviceWorker.register('/sw.js');"));
    }
}
