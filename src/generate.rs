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
}

#[derive(Debug, Deserialize)]
pub struct AboutPage {
    pub title: String,
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

const CSS: &str = include_str!("../static/style.css");
const JS: &str = include_str!("../static/nav.js");

pub fn generate(
    manifest_path: &Path,
    processed_dir: &Path,
    output_dir: &Path,
) -> Result<(), GenerateError> {
    let manifest_content = fs::read_to_string(manifest_path)?;
    let manifest: Manifest = serde_json::from_str(&manifest_content)?;

    fs::create_dir_all(output_dir)?;

    // Copy processed images to output
    copy_dir_recursive(processed_dir, output_dir)?;

    // Generate index page
    let index_html = generate_index(&manifest);
    fs::write(output_dir.join("index.html"), index_html)?;
    println!("Generated index.html");

    // Generate about page if present
    if let Some(about) = &manifest.about {
        let about_html = generate_about_page(about, &manifest.navigation);
        fs::write(output_dir.join("about.html"), about_html)?;
        println!("Generated about.html");
    }

    // Generate album pages
    for album in &manifest.albums {
        let album_dir = output_dir.join(&album.path);
        fs::create_dir_all(&album_dir)?;

        let has_about = manifest.about.is_some();
        let album_html = generate_album_page(album, &manifest.navigation, has_about);
        fs::write(album_dir.join("index.html"), album_html)?;
        println!("Generated {}/index.html", album.path);

        // Generate image pages
        for (idx, image) in album.images.iter().enumerate() {
            let prev = if idx > 0 {
                Some(&album.images[idx - 1])
            } else {
                None
            };
            let next = album.images.get(idx + 1);

            let image_html = generate_image_page(album, image, prev, next, &manifest.navigation, has_about);
            let image_filename = format!("{}.html", idx + 1);
            fs::write(album_dir.join(&image_filename), image_html)?;
        }
        println!("Generated {} image pages for {}", album.images.len(), album.title);
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

fn generate_index(manifest: &Manifest) -> String {
    let nav_html = generate_nav(&manifest.navigation, "", manifest.about.is_some());

    let albums_html: String = manifest
        .albums
        .iter()
        .filter(|a| a.in_nav)
        .map(|album| {
            format!(
                r#"<a href="{path}/" class="album-card">
                    <img src="{thumb}" alt="{title}" loading="lazy">
                    <span class="album-title">{title}</span>
                </a>"#,
                path = album.path,
                thumb = album.thumbnail,
                title = html_escape(&album.title),
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Gallery</title>
    <style>{css}</style>
</head>
<body>
    <nav class="site-nav">
        {nav}
    </nav>
    <main class="index-page">
        <div class="album-grid">
            {albums}
        </div>
    </main>
</body>
</html>"#,
        css = CSS,
        nav = nav_html,
        albums = albums_html,
    )
}

fn generate_album_page(album: &Album, navigation: &[NavItem], has_about: bool) -> String {
    let nav_html = generate_nav(navigation, &album.path, has_about);

    // Strip album path prefix since album page is inside the album directory
    let strip_prefix = |path: &str| -> String {
        path.strip_prefix(&album.path)
            .and_then(|p| p.strip_prefix('/'))
            .unwrap_or(path)
            .to_string()
    };

    let description_html = album
        .description
        .as_ref()
        .map(|d| format!(r#"<p class="album-description">{}</p>"#, html_escape(d)))
        .unwrap_or_default();

    let thumbnails_html: String = album
        .images
        .iter()
        .enumerate()
        .map(|(idx, image)| {
            format!(
                r#"<a href="{}.html" class="thumb-link">
                    <img src="{}" alt="Image {}" loading="lazy">
                </a>"#,
                idx + 1,
                strip_prefix(&image.thumbnail),
                idx + 1,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{title}</title>
    <style>{css}</style>
</head>
<body>
    <nav class="site-nav">
        {nav}
    </nav>
    <main class="album-page">
        <header class="album-header">
            <h1>{title}</h1>
            {description}
        </header>
        <div class="thumbnail-grid">
            {thumbnails}
        </div>
    </main>
</body>
</html>"#,
        title = html_escape(&album.title),
        css = CSS,
        nav = nav_html,
        description = description_html,
        thumbnails = thumbnails_html,
    )
}

fn generate_image_page(
    album: &Album,
    image: &Image,
    prev: Option<&Image>,
    next: Option<&Image>,
    navigation: &[NavItem],
    has_about: bool,
) -> String {
    let nav_html = generate_nav(navigation, &album.path, has_about);

    // Strip album path prefix since image pages are inside the album directory
    let strip_prefix = |path: &str| -> String {
        path.strip_prefix(&album.path)
            .and_then(|p| p.strip_prefix('/'))
            .unwrap_or(path)
            .to_string()
    };

    // Get the largest generated size for srcset
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

    // Calculate aspect ratio for the frame
    let (width, height) = image.dimensions;
    let aspect_ratio = width as f64 / height as f64;

    // Navigation URLs
    let prev_url = prev
        .map(|_| {
            let idx = album.images.iter().position(|i| i.number == image.number).unwrap();
            format!("{}.html", idx) // idx is already 0-based, prev would be idx (since enumerate is 1-based in filename)
        })
        .unwrap_or_else(|| "index.html".to_string());

    let next_url = next
        .map(|_| {
            let idx = album.images.iter().position(|i| i.number == image.number).unwrap();
            format!("{}.html", idx + 2)
        })
        .unwrap_or_else(|| "index.html".to_string());

    let image_idx = album.images.iter().position(|i| i.number == image.number).unwrap() + 1;

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{album_title} - {idx}</title>
    <style>{css}</style>
</head>
<body class="image-view">
    <nav class="site-nav">
        {nav}
    </nav>
    <nav class="breadcrumb">
        <a href="/">Home</a> &rsaquo; <a href="index.html">{album_title}</a>
    </nav>
    <main class="image-page">
        <figure class="image-frame" style="--aspect-ratio: {aspect_ratio};">
            <picture>
                <source type="image/avif" srcset="{srcset_avif}" sizes="(max-width: 800px) 100vw, 80vw">
                <source type="image/webp" srcset="{srcset_webp}" sizes="(max-width: 800px) 100vw, 80vw">
                <img src="{default_src}" alt="{album_title} - Image {idx}">
            </picture>
        </figure>
    </main>
    <div class="nav-zones" data-prev="{prev_url}" data-next="{next_url}"></div>
    <script>{js}</script>
</body>
</html>"#,
        album_title = html_escape(&album.title),
        idx = image_idx,
        css = CSS,
        nav = nav_html,
        aspect_ratio = aspect_ratio,
        srcset_avif = srcset_avif,
        srcset_webp = srcset_webp,
        default_src = default_src,
        prev_url = prev_url,
        next_url = next_url,
        js = JS,
    )
}

fn generate_nav(items: &[NavItem], current_path: &str, has_about: bool) -> String {
    let items_html: String = items
        .iter()
        .map(|item| {
            let is_current = item.path == current_path || current_path.starts_with(&format!("{}/", item.path));
            let class = if is_current { r#" class="current""# } else { "" };

            if item.children.is_empty() {
                format!(
                    r#"<li{class}><a href="/{path}/">{title}</a></li>"#,
                    class = class,
                    path = item.path,
                    title = html_escape(&item.title),
                )
            } else {
                let children_html = generate_nav_list(&item.children, current_path);
                format!(
                    r#"<li{class}>
                        <span class="nav-group">{title}</span>
                        {children}
                    </li>"#,
                    class = class,
                    title = html_escape(&item.title),
                    children = children_html,
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Add about link if present
    let about_html = if has_about {
        let class = if current_path == "about" { r#" class="current""# } else { "" };
        format!(r#"<li{class}><a href="/about.html">About</a></li>"#, class = class)
    } else {
        String::new()
    };

    // Wrap in details/summary for collapsible menu
    format!(
        r#"<details class="nav-menu">
            <summary>Menu</summary>
            <ul>{items}{about}</ul>
        </details>"#,
        items = items_html,
        about = about_html,
    )
}

fn generate_nav_list(items: &[NavItem], current_path: &str) -> String {
    let items_html: String = items
        .iter()
        .map(|item| {
            let is_current = item.path == current_path || current_path.starts_with(&format!("{}/", item.path));
            let class = if is_current { r#" class="current""# } else { "" };

            if item.children.is_empty() {
                format!(
                    r#"<li{class}><a href="/{path}/">{title}</a></li>"#,
                    class = class,
                    path = item.path,
                    title = html_escape(&item.title),
                )
            } else {
                let children_html = generate_nav_list(&item.children, current_path);
                format!(
                    r#"<li{class}>
                        <span class="nav-group">{title}</span>
                        {children}
                    </li>"#,
                    class = class,
                    title = html_escape(&item.title),
                    children = children_html,
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(r#"<ul>{}</ul>"#, items_html)
}

fn generate_about_page(about: &AboutPage, navigation: &[NavItem]) -> String {
    let nav_html = generate_nav(navigation, "about", true);

    // Convert newlines to paragraphs
    let body_html: String = about
        .body
        .split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .map(|p| format!("<p>{}</p>", html_escape(p.trim())))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{title}</title>
    <style>{css}</style>
</head>
<body>
    <nav class="site-nav">
        {nav}
    </nav>
    <main class="about-page">
        <article class="about-content">
            <h1>{title}</h1>
            {body}
        </article>
    </main>
</body>
</html>"#,
        title = html_escape(&about.title),
        css = CSS,
        nav = nav_html,
        body = body_html,
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_escape_works() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("A & B"), "A &amp; B");
    }

    #[test]
    fn nav_generation() {
        let items = vec![
            NavItem {
                title: "Album One".to_string(),
                path: "010-one".to_string(),
                children: vec![],
            },
        ];
        let html = generate_nav(&items, "", false);
        assert!(html.contains("Album One"));
        assert!(html.contains("/010-one/"));
    }

    #[test]
    fn nav_includes_about_when_present() {
        let items = vec![];
        let html = generate_nav(&items, "", true);
        assert!(html.contains("About"));
        assert!(html.contains("/about.html"));
    }
}
