//! HTML site generation.
//!
//! Stage 3 of the Simple Gal build pipeline. Takes the processed manifest and
//! generates the final static HTML site.
//!
//! ## Generated Pages
//!
//! - **Index page** (`/index.html`): Album grid showing thumbnails of all albums
//! - **Album pages** (`/{album}/index.html`): Thumbnail grid for an album
//! - **Image pages** (`/{album}/{n}.html`): Full-screen image viewer with navigation
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
//! ├── 010-Landscapes/
//! │   ├── index.html             # Album page
//! │   ├── 1.html                 # Image viewer pages
//! │   ├── 2.html
//! │   ├── 001-dawn-800.avif      # Processed images (copied)
//! │   ├── 001-dawn-800.webp
//! │   └── ...
//! └── 020-Travel/
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
#[allow(dead_code)]
pub struct Manifest {
    pub navigation: Vec<NavItem>,
    pub albums: Vec<Album>,
    #[serde(default)]
    pub pages: Vec<Page>,
    pub config: SiteConfig,
}

#[derive(Debug, Deserialize, Clone)]
#[allow(dead_code)]
pub struct Page {
    pub title: String,
    pub link_title: String,
    pub slug: String,
    pub body: String,
    pub in_nav: bool,
    pub sort_key: u32,
    pub is_link: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NavItem {
    pub title: String,
    pub path: String,
    #[serde(default)]
    pub children: Vec<NavItem>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Album {
    pub path: String,
    pub title: String,
    pub description: Option<String>,
    pub thumbnail: String,
    pub images: Vec<Image>,
    pub in_nav: bool,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct Image {
    pub number: u32,
    pub source_path: String,
    #[serde(default)]
    pub title: Option<String>,
    pub dimensions: (u32, u32),
    pub generated: BTreeMap<String, GeneratedVariant>,
    pub thumbnail: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct GeneratedVariant {
    pub avif: String,
    pub webp: String,
    pub width: u32,
    pub height: u32,
}

const CSS_STATIC: &str = include_str!("../static/style.css");
const JS: &str = include_str!("../static/nav.js");

pub fn generate(
    manifest_path: &Path,
    processed_dir: &Path,
    output_dir: &Path,
) -> Result<(), GenerateError> {
    let manifest_content = fs::read_to_string(manifest_path)?;
    let manifest: Manifest = serde_json::from_str(&manifest_content)?;

    // Generate CSS: @import must come first, then config variables, then static rules
    let font_import = "@import url('https://fonts.googleapis.com/css2?family=Libre+Franklin:wght@500&display=swap');";
    let color_css = config::generate_color_css(&manifest.config.colors);
    let theme_css = config::generate_theme_css(&manifest.config.theme);
    let css = format!(
        "{}\n\n{}\n\n{}\n\n{}",
        font_import, color_css, theme_css, CSS_STATIC
    );

    fs::create_dir_all(output_dir)?;

    // Copy processed images to output
    copy_dir_recursive(processed_dir, output_dir)?;

    // Generate index page
    let index_html = render_index(&manifest, &css);
    fs::write(output_dir.join("index.html"), index_html.into_string())?;
    println!("Generated index.html");

    // Generate pages (content pages only, not link pages)
    for page in manifest.pages.iter().filter(|p| !p.is_link) {
        let page_html = render_page(page, &manifest.navigation, &manifest.pages, &css);
        let filename = format!("{}.html", page.slug);
        fs::write(output_dir.join(&filename), page_html.into_string())?;
        println!("Generated {}", filename);
    }

    // Generate album pages
    for album in &manifest.albums {
        let album_dir = output_dir.join(&album.path);
        fs::create_dir_all(&album_dir)?;

        let album_html = render_album_page(album, &manifest.navigation, &manifest.pages, &css);
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
            );
            let image_filename = format!("{}.html", idx + 1);
            fs::write(album_dir.join(&image_filename), image_html.into_string())?;
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

/// Renders the base HTML document structure
fn base_document(title: &str, css: &str, body_class: Option<&str>, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { (title) }
                style { (PreEscaped(css)) }
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
fn render_index(manifest: &Manifest, css: &str) -> Markup {
    let nav = render_nav(&manifest.navigation, "", &manifest.pages);

    let breadcrumb = html! {
        a href="/" { "Gallery" }
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

    base_document("Gallery", css, None, content)
}

/// Renders an album page with thumbnail grid
fn render_album_page(album: &Album, navigation: &[NavItem], pages: &[Page], css: &str) -> Markup {
    let nav = render_nav(navigation, &album.path, pages);

    let breadcrumb = html! {
        a href="/" { "Gallery" }
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
                    p.album-description { (desc) }
                }
            }
            div.thumbnail-grid {
                @for (idx, image) in album.images.iter().enumerate() {
                    a.thumb-link href={ (idx + 1) ".html" } {
                        img src=(strip_prefix(&image.thumbnail)) alt={ "Image " (idx + 1) } loading="lazy";
                    }
                }
            }
        }
    };

    base_document(&album.title, css, None, content)
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
    let width = match total {
        0..=9 => 1,
        10..=99 => 2,
        100..=999 => 3,
        _ => 4,
    };
    match title {
        Some(t) => format!("{:0>width$}. {}", position, t),
        None => format!("{:0>width$}", position),
    }
}

/// Renders an image viewer page
fn render_image_page(
    album: &Album,
    image: &Image,
    prev: Option<&Image>,
    next: Option<&Image>,
    navigation: &[NavItem],
    pages: &[Page],
    css: &str,
) -> Markup {
    let nav = render_nav(navigation, &album.path, pages);

    // Strip album path prefix since image pages are inside the album directory
    let strip_prefix = |path: &str| -> String {
        path.strip_prefix(&album.path)
            .and_then(|p| p.strip_prefix('/'))
            .unwrap_or(path)
            .to_string()
    };

    // Build srcsets
    let sizes: Vec<_> = image.generated.iter().collect();

    let srcset_avif: String = sizes
        .iter()
        .map(|(size, variant)| format!("{} {}w", strip_prefix(&variant.avif), size))
        .collect::<Vec<_>>()
        .join(", ");

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

    // Calculate aspect ratio
    let (width, height) = image.dimensions;
    let aspect_ratio = width as f64 / height as f64;

    // Navigation URLs
    let image_idx = album
        .images
        .iter()
        .position(|i| i.number == image.number)
        .unwrap();

    let prev_url = if prev.is_some() {
        format!("{}.html", image_idx) // image_idx is 0-based, filename is 1-based
    } else {
        "index.html".to_string()
    };

    let next_url = if next.is_some() {
        format!("{}.html", image_idx + 2)
    } else {
        "index.html".to_string()
    };

    let display_idx = image_idx + 1;
    let image_label = format_image_label(display_idx, album.images.len(), image.title.as_deref());
    let page_title = format!("{} - {}", album.title, image_label);

    let breadcrumb = html! {
        a href="/" { "Gallery" }
        " › "
        a href="index.html" { (album.title) }
        " › "
        (image_label)
    };

    let aspect_style = format!("--aspect-ratio: {};", aspect_ratio);
    let alt_text = match &image.title {
        Some(t) => format!("{} - {}", album.title, t),
        None => format!("{} - Image {}", album.title, display_idx),
    };

    let content = html! {
        (site_header(breadcrumb, nav))
        main {
            div.image-page {
                figure.image-frame style=(aspect_style) {
                    picture {
                        source type="image/avif" srcset=(srcset_avif) sizes="(max-width: 800px) 100vw, 80vw";
                        source type="image/webp" srcset=(srcset_webp) sizes="(max-width: 800px) 100vw, 80vw";
                        img src=(default_src) alt=(alt_text);
                    }
                }
            }
        }
        div.nav-zones data-prev=(prev_url) data-next=(next_url) {}
        script { (PreEscaped(JS)) }
    };

    base_document(&page_title, css, Some("image-view"), content)
}

/// Renders a content page from markdown
fn render_page(page: &Page, navigation: &[NavItem], pages: &[Page], css: &str) -> Markup {
    let nav = render_nav(navigation, &page.slug, pages);

    // Convert markdown to HTML
    let parser = Parser::new(&page.body);
    let mut body_html = String::new();
    md_html::push_html(&mut body_html, parser);

    let breadcrumb = html! {
        a href="/" { "Gallery" }
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

    base_document(&page.title, css, None, content)
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
        let doc = base_document("Test", "body {}", None, content).into_string();
        assert!(doc.starts_with("<!DOCTYPE html>"));
    }

    #[test]
    fn base_document_applies_body_class() {
        let content = html! { p { "test" } };
        let doc = base_document("Test", "", Some("image-view"), content).into_string();
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
            path: "010-test".to_string(),
            title: "Test Album".to_string(),
            description: Some("A test album description".to_string()),
            thumbnail: "010-test/001-image-thumb.webp".to_string(),
            images: vec![
                Image {
                    number: 1,
                    source_path: "010-test/001-image.jpg".to_string(),
                    title: Some("Dawn".to_string()),
                    dimensions: (1600, 1200),
                    generated: {
                        let mut map = BTreeMap::new();
                        map.insert(
                            "800".to_string(),
                            GeneratedVariant {
                                avif: "010-test/001-image-800.avif".to_string(),
                                webp: "010-test/001-image-800.webp".to_string(),
                                width: 800,
                                height: 600,
                            },
                        );
                        map.insert(
                            "1400".to_string(),
                            GeneratedVariant {
                                avif: "010-test/001-image-1400.avif".to_string(),
                                webp: "010-test/001-image-1400.webp".to_string(),
                                width: 1400,
                                height: 1050,
                            },
                        );
                        map
                    },
                    thumbnail: "010-test/001-image-thumb.webp".to_string(),
                },
                Image {
                    number: 2,
                    source_path: "010-test/002-image.jpg".to_string(),
                    title: None,
                    dimensions: (1200, 1600),
                    generated: {
                        let mut map = BTreeMap::new();
                        map.insert(
                            "800".to_string(),
                            GeneratedVariant {
                                avif: "010-test/002-image-800.avif".to_string(),
                                webp: "010-test/002-image-800.webp".to_string(),
                                width: 600,
                                height: 800,
                            },
                        );
                        map
                    },
                    thumbnail: "010-test/002-image-thumb.webp".to_string(),
                },
            ],
            in_nav: true,
        }
    }

    #[test]
    fn render_album_page_includes_title() {
        let album = create_test_album();
        let nav = vec![];
        let html = render_album_page(&album, &nav, &[], "").into_string();

        assert!(html.contains("Test Album"));
        assert!(html.contains("<h1>"));
    }

    #[test]
    fn render_album_page_includes_description() {
        let album = create_test_album();
        let nav = vec![];
        let html = render_album_page(&album, &nav, &[], "").into_string();

        assert!(html.contains("A test album description"));
        assert!(html.contains("album-description"));
    }

    #[test]
    fn render_album_page_thumbnail_links() {
        let album = create_test_album();
        let nav = vec![];
        let html = render_album_page(&album, &nav, &[], "").into_string();

        // Should have links to image pages (1.html, 2.html)
        assert!(html.contains("1.html"));
        assert!(html.contains("2.html"));
        // Thumbnails should have paths relative to album dir
        assert!(html.contains("001-image-thumb.webp"));
    }

    #[test]
    fn render_album_page_breadcrumb() {
        let album = create_test_album();
        let nav = vec![];
        let html = render_album_page(&album, &nav, &[], "").into_string();

        // Breadcrumb should link to gallery root
        assert!(html.contains(r#"href="/""#));
        assert!(html.contains("Gallery"));
    }

    #[test]
    fn render_image_page_includes_picture_element() {
        let album = create_test_album();
        let image = &album.images[0];
        let nav = vec![];
        let html = render_image_page(&album, image, None, Some(&album.images[1]), &nav, &[], "")
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
        let html = render_image_page(&album, image, None, Some(&album.images[1]), &nav, &[], "")
            .into_string();

        // Should have srcset with sizes
        assert!(html.contains("srcset="));
        assert!(html.contains("800w"));
        assert!(html.contains("1400w"));
    }

    #[test]
    fn render_image_page_navigation_zones() {
        let album = create_test_album();
        let image = &album.images[0];
        let nav = vec![];
        let html = render_image_page(&album, image, None, Some(&album.images[1]), &nav, &[], "")
            .into_string();

        assert!(html.contains("nav-zones"));
        assert!(html.contains("data-prev="));
        assert!(html.contains("data-next="));
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
        )
        .into_string();
        assert!(html1.contains(r#"data-prev="index.html""#));
        assert!(html1.contains(r#"data-next="2.html""#));

        // Second image - has prev, no next
        let html2 = render_image_page(
            &album,
            &album.images[1],
            Some(&album.images[0]),
            None,
            &nav,
            &[],
            "",
        )
        .into_string();
        assert!(html2.contains(r#"data-prev="1.html""#));
        assert!(html2.contains(r#"data-next="index.html""#));
    }

    #[test]
    fn render_image_page_aspect_ratio() {
        let album = create_test_album();
        let image = &album.images[0]; // 1600x1200 = 1.333...
        let nav = vec![];
        let html = render_image_page(&album, image, None, None, &nav, &[], "").into_string();

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
        let html = render_page(&page, &[], &[], "").into_string();

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
        let html = render_page(&page, &[], &[], "").into_string();

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
        let html = render_image_page(&album, image, None, Some(&album.images[1]), &nav, &[], "")
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
        let html = render_image_page(&album, image, Some(&album.images[0]), None, &nav, &[], "")
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
        let html = render_image_page(&album, image, None, Some(&album.images[1]), &nav, &[], "")
            .into_string();

        assert!(html.contains("<title>Test Album - 1. Dawn</title>"));
    }

    #[test]
    fn image_alt_text_uses_title() {
        let album = create_test_album();
        let image = &album.images[0]; // has title "Dawn"
        let nav = vec![];
        let html = render_image_page(&album, image, None, Some(&album.images[1]), &nav, &[], "")
            .into_string();

        assert!(html.contains("Test Album - Dawn"));
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
}
