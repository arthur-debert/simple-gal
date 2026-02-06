//! HTML site generation.
//!
//! Stage 3 of the LightTable build pipeline. Takes the processed manifest and
//! generates the final static HTML site.
//!
//! ## Generated Pages
//!
//! - **Index page** (`/index.html`): Album grid showing thumbnails of all albums
//! - **Album pages** (`/{album}/index.html`): Thumbnail grid for an album
//! - **Image pages** (`/{album}/{n}.html`): Full-screen image viewer with navigation
//! - **About page** (`/about.html`): Optional markdown content converted to HTML
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
//! ├── about.html                 # About page (if about.md exists)
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
    pub about: Option<AboutPage>,
    pub config: SiteConfig,
}

#[derive(Debug, Deserialize)]
pub struct AboutPage {
    /// Title from markdown content (first # heading)
    pub title: String,
    /// Link title from filename (dashes to spaces)
    pub link_title: String,
    /// Raw markdown body content
    pub body: String,
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

    // Generate CSS with colors from config
    let color_css = config::generate_color_css(&manifest.config.colors);
    let css = format!("{}\n\n{}", color_css, CSS_STATIC);

    fs::create_dir_all(output_dir)?;

    // Copy processed images to output
    copy_dir_recursive(processed_dir, output_dir)?;

    // Generate index page
    let index_html = render_index(&manifest, &css);
    fs::write(output_dir.join("index.html"), index_html.into_string())?;
    println!("Generated index.html");

    // Generate about page if present
    if let Some(about) = &manifest.about {
        let about_html = render_about_page(about, &manifest.navigation, &css);
        fs::write(output_dir.join("about.html"), about_html.into_string())?;
        println!("Generated about.html");
    }

    // Generate album pages
    let about_link_title = manifest.about.as_ref().map(|a| a.link_title.as_str());
    for album in &manifest.albums {
        let album_dir = output_dir.join(&album.path);
        fs::create_dir_all(&album_dir)?;

        let album_html = render_album_page(album, &manifest.navigation, about_link_title, &css);
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
                about_link_title,
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
                style { (css) }
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

/// Renders the navigation menu (hamburger style, slides from right)
pub fn render_nav(items: &[NavItem], current_path: &str, about_link_title: Option<&str>) -> Markup {
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
                // Separator and page links
                li.nav-separator role="separator" {}
                @if let Some(link_title) = about_link_title {
                    @let is_current = current_path == "about";
                    li class=[is_current.then_some("current")] {
                        a href="/about.html" { (link_title) }
                    }
                }
                li {
                    a href="https://github.com/arthur-debert/websets" target="_blank" rel="noopener" {
                        "GitHub"
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
    let about_link_title = manifest.about.as_ref().map(|a| a.link_title.as_str());
    let nav = render_nav(&manifest.navigation, "", about_link_title);

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
fn render_album_page(
    album: &Album,
    navigation: &[NavItem],
    about_link_title: Option<&str>,
    css: &str,
) -> Markup {
    let nav = render_nav(navigation, &album.path, about_link_title);

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

/// Renders an image viewer page
fn render_image_page(
    album: &Album,
    image: &Image,
    prev: Option<&Image>,
    next: Option<&Image>,
    navigation: &[NavItem],
    about_link_title: Option<&str>,
    css: &str,
) -> Markup {
    let nav = render_nav(navigation, &album.path, about_link_title);

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
    let page_title = format!("{} - {}", album.title, display_idx);

    let breadcrumb = html! {
        a href="/" { "Gallery" }
        " › "
        a href="index.html" { (album.title) }
    };

    let aspect_style = format!("--aspect-ratio: {};", aspect_ratio);
    let alt_text = format!("{} - Image {}", album.title, display_idx);

    let content = html! {
        (site_header(breadcrumb, nav))
        main.image-page {
            figure.image-frame style=(aspect_style) {
                picture {
                    source type="image/avif" srcset=(srcset_avif) sizes="(max-width: 800px) 100vw, 80vw";
                    source type="image/webp" srcset=(srcset_webp) sizes="(max-width: 800px) 100vw, 80vw";
                    img src=(default_src) alt=(alt_text);
                }
            }
        }
        div.nav-zones data-prev=(prev_url) data-next=(next_url) {}
        script { (PreEscaped(JS)) }
    };

    base_document(&page_title, css, Some("image-view"), content)
}

/// Renders the about page from markdown content
fn render_about_page(about: &AboutPage, navigation: &[NavItem], css: &str) -> Markup {
    let nav = render_nav(navigation, "about", Some(&about.link_title));

    // Convert markdown to HTML
    let parser = Parser::new(&about.body);
    let mut body_html = String::new();
    md_html::push_html(&mut body_html, parser);

    let breadcrumb = html! {
        a href="/" { "Gallery" }
        " › "
        (about.title)
    };

    let content = html! {
        (site_header(breadcrumb, nav))
        main.about-page {
            article.about-content {
                (PreEscaped(body_html))
            }
        }
    };

    base_document(&about.title, css, None, content)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nav_renders_items() {
        let items = vec![NavItem {
            title: "Album One".to_string(),
            path: "010-one".to_string(),
            children: vec![],
        }];
        let html = render_nav(&items, "", None).into_string();
        assert!(html.contains("Album One"));
        assert!(html.contains("/010-one/"));
    }

    #[test]
    fn nav_includes_about_when_present() {
        let items = vec![];
        let html = render_nav(&items, "", Some("About")).into_string();
        assert!(html.contains("About"));
        assert!(html.contains("/about.html"));
    }

    #[test]
    fn nav_uses_custom_about_link_title() {
        let items = vec![];
        let html = render_nav(&items, "", Some("who am i")).into_string();
        assert!(html.contains("who am i"));
        assert!(html.contains("/about.html"));
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
        let html = render_nav(&items, "020-second", None).into_string();
        // The second item should have the current class
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
        let html = render_nav(&items, "", None).into_string();
        assert!(html.contains("Parent"));
        assert!(html.contains("Child"));
        assert!(html.contains("nav-group")); // Parent should have nav-group class
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
        let html = render_album_page(&album, &nav, None, "").into_string();

        assert!(html.contains("Test Album"));
        assert!(html.contains("<h1>"));
    }

    #[test]
    fn render_album_page_includes_description() {
        let album = create_test_album();
        let nav = vec![];
        let html = render_album_page(&album, &nav, None, "").into_string();

        assert!(html.contains("A test album description"));
        assert!(html.contains("album-description"));
    }

    #[test]
    fn render_album_page_thumbnail_links() {
        let album = create_test_album();
        let nav = vec![];
        let html = render_album_page(&album, &nav, None, "").into_string();

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
        let html = render_album_page(&album, &nav, None, "").into_string();

        // Breadcrumb should link to gallery root
        assert!(html.contains(r#"href="/""#));
        assert!(html.contains("Gallery"));
    }

    #[test]
    fn render_image_page_includes_picture_element() {
        let album = create_test_album();
        let image = &album.images[0];
        let nav = vec![];
        let html = render_image_page(&album, image, None, Some(&album.images[1]), &nav, None, "")
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
        let html = render_image_page(&album, image, None, Some(&album.images[1]), &nav, None, "")
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
        let html = render_image_page(&album, image, None, Some(&album.images[1]), &nav, None, "")
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
            None,
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
            None,
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
        let html = render_image_page(&album, image, None, None, &nav, None, "").into_string();

        // Should have aspect ratio CSS variable
        assert!(html.contains("--aspect-ratio:"));
    }

    #[test]
    fn render_about_page_converts_markdown() {
        let about = AboutPage {
            title: "About Me".to_string(),
            link_title: "about".to_string(),
            body: "# About Me\n\nThis is **bold** and *italic*.".to_string(),
        };
        let nav = vec![];
        let html = render_about_page(&about, &nav, "").into_string();

        // Markdown should be converted to HTML
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<em>italic</em>"));
    }

    #[test]
    fn render_about_page_includes_title() {
        let about = AboutPage {
            title: "About Me".to_string(),
            link_title: "about me".to_string(),
            body: "Content here".to_string(),
        };
        let nav = vec![];
        let html = render_about_page(&about, &nav, "").into_string();

        assert!(html.contains("<title>About Me</title>"));
        assert!(html.contains("about-page"));
    }

    #[test]
    fn html_escape_in_maud() {
        // Maud should automatically escape HTML in content
        let items = vec![NavItem {
            title: "<script>alert('xss')</script>".to_string(),
            path: "test".to_string(),
            children: vec![],
        }];
        let html = render_nav(&items, "", None).into_string();

        // Should be escaped, not raw script tag
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&lt;script&gt;"));
    }
}
