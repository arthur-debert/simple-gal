//! Integration tests for cross-backend dimension parity and visual comparison.
//!
//! `cross_backend_dimension_parity` runs automatically with synthetic images
//! and asserts that ImageMagick and Rust backends produce identical output
//! dimensions. Skips gracefully when ImageMagick is not installed.
//!
//! `generate_comparison_images` is `#[ignore]` — requires real content images
//! for manual quality inspection:
//!   cargo test --test compare_backends generate_comparison_images -- --ignored --nocapture
//!
//! Outputs go to /tmp/simple-gal-compare/

// Since simple-gal is a binary crate, we can't import from it directly.
// Instead, this test uses the image/webp crates directly to replicate
// what RustBackend does, and shells out to ImageMagick for comparison.

use image::{ImageEncoder, RgbImage};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

const OUTPUT_DIR: &str = "/tmp/simple-gal-compare";
const SIZES: &[u32] = &[800, 1400, 2080];
const QUALITY: u32 = 90;
const THUMB_CROP_W: u32 = 400;
const THUMB_CROP_H: u32 = 500;

/// Create a valid JPEG file with the given dimensions.
///
/// Duplicated from rust_backend tests because simple-gal is a binary crate.
fn create_test_jpeg(path: &Path, width: u32, height: u32) {
    let img = RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
    });
    let file = std::fs::File::create(path).unwrap();
    let writer = std::io::BufWriter::new(file);
    image::codecs::jpeg::JpegEncoder::new(writer)
        .write_image(img.as_raw(), width, height, image::ExtendedColorType::Rgb8)
        .unwrap();
}

fn magick_available() -> bool {
    Command::new("convert")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn magick_dimensions(path: &Path) -> (u32, u32) {
    let out = Command::new("identify")
        .args(["-format", "%wx%h", path.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = String::from_utf8_lossy(&out.stdout);
    let parts: Vec<&str> = s.trim().split('x').collect();
    (parts[0].parse().unwrap(), parts[1].parse().unwrap())
}

fn magick_resize(source: &Path, output: &Path, w: u32, h: u32, quality: u32) {
    let size = format!("{}x{}", w, h);
    let q = quality.to_string();
    let mut args: Vec<&str> = vec![source.to_str().unwrap(), "-resize", &size, "-quality", &q];
    let heic_speed;
    if output.extension().and_then(|e| e.to_str()) == Some("avif") {
        heic_speed = "heic:speed=6".to_string();
        args.push("-define");
        args.push(&heic_speed);
    }
    args.push(output.to_str().unwrap());
    let out = Command::new("convert").args(&args).output().unwrap();
    assert!(
        out.status.success(),
        "convert failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn magick_thumbnail(source: &Path, output: &Path, crop_w: u32, crop_h: u32, quality: u32) {
    let fill_size = format!("{}x{}^", crop_w, crop_h);
    let crop_size = format!("{}x{}", crop_w, crop_h);
    let q = quality.to_string();
    let sharpen = "0x0.5";
    let out = Command::new("convert")
        .args([
            source.to_str().unwrap(),
            "-resize",
            &fill_size,
            "-gravity",
            "center",
            "-extent",
            &crop_size,
            "-quality",
            &q,
            "-sharpen",
            sharpen,
            output.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "convert failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn rust_resize(source: &Path, output: &Path, w: u32, h: u32, quality: u32) {
    let img = image::ImageReader::open(source).unwrap().decode().unwrap();
    let resized = img.resize(w, h, image::imageops::FilterType::Lanczos3);
    save_image(&resized, output, quality);
}

fn rust_thumbnail(source: &Path, output: &Path, crop_w: u32, crop_h: u32, quality: u32) {
    let img = image::ImageReader::open(source).unwrap().decode().unwrap();
    let filled = img.resize_to_fill(crop_w, crop_h, image::imageops::FilterType::Lanczos3);
    let sharpened = image::DynamicImage::from(image::imageops::unsharpen(&filled, 0.5, 0));
    save_image(&sharpened, output, quality);
}

fn save_image(img: &image::DynamicImage, path: &Path, quality: u32) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "webp" => {
            let encoder = webp::Encoder::from_image(img).unwrap();
            let encoded = encoder.encode(quality as f32);
            std::fs::write(path, &*encoded).unwrap();
        }
        "avif" => {
            let file = std::fs::File::create(path).unwrap();
            let writer = std::io::BufWriter::new(file);
            let encoder =
                image::codecs::avif::AvifEncoder::new_with_speed_quality(writer, 6, quality as u8);
            img.write_with_encoder(encoder).unwrap();
        }
        _ => panic!("Unsupported format: {}", ext),
    }
}

fn file_kb(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len() / 1024).unwrap_or(0)
}

/// Automated parity test: synthetic images through both backends, assert dimensions match.
#[test]
fn cross_backend_dimension_parity() {
    if !magick_available() {
        eprintln!("ImageMagick not found - skipping cross-backend parity test");
        return;
    }

    let tmp = tempfile::TempDir::new().unwrap();

    // Resize cases: (source_w, source_h, target_w, target_h)
    let resize_cases = [
        (800, 600, 400, 300),
        (600, 800, 300, 400),
        (1920, 1080, 800, 450),
        (500, 500, 200, 200),
    ];

    for (sw, sh, rw, rh) in resize_cases {
        let source = tmp.path().join(format!("resize_src_{}x{}.jpg", sw, sh));
        create_test_jpeg(&source, sw, sh);

        let magick_out = tmp
            .path()
            .join(format!("magick_resize_{}x{}_to_{}x{}.webp", sw, sh, rw, rh));
        let rust_out = tmp
            .path()
            .join(format!("rust_resize_{}x{}_to_{}x{}.webp", sw, sh, rw, rh));

        magick_resize(&source, &magick_out, rw, rh, QUALITY);
        rust_resize(&source, &rust_out, rw, rh, QUALITY);

        let magick_dims = magick_dimensions(&magick_out);
        let rust_dims = image::image_dimensions(&rust_out).unwrap();

        assert_eq!(
            magick_dims, rust_dims,
            "Resize dimension mismatch for {}x{} → {}x{}: magick={:?}, rust={:?}",
            sw, sh, rw, rh, magick_dims, rust_dims
        );
    }

    // Thumbnail cases: (source_w, source_h, crop_w, crop_h)
    let thumb_cases = [
        (800, 600, 400, 500),
        (600, 800, 400, 500),
        (1000, 1000, 400, 500),
        (400, 300, 200, 200),
    ];

    for (sw, sh, cw, ch) in thumb_cases {
        let source = tmp.path().join(format!("thumb_src_{}x{}.jpg", sw, sh));
        create_test_jpeg(&source, sw, sh);

        let magick_out = tmp
            .path()
            .join(format!("magick_thumb_{}x{}_{}x{}.webp", sw, sh, cw, ch));
        let rust_out = tmp
            .path()
            .join(format!("rust_thumb_{}x{}_{}x{}.webp", sw, sh, cw, ch));

        magick_thumbnail(&source, &magick_out, cw, ch, QUALITY);
        rust_thumbnail(&source, &rust_out, cw, ch, QUALITY);

        let magick_dims = magick_dimensions(&magick_out);
        let rust_dims = image::image_dimensions(&rust_out).unwrap();

        assert_eq!(
            magick_dims, rust_dims,
            "Thumbnail dimension mismatch for {}x{} → crop {}x{}: magick={:?}, rust={:?}",
            sw, sh, cw, ch, magick_dims, rust_dims
        );
    }
}

#[test]
#[ignore]
fn generate_comparison_images() {
    let test_images: Vec<PathBuf> = vec![
        "content/001-NY/Q1020899.jpg".into(),
        "content/002-Greece/L1000601.jpg".into(),
        "content/001-NY/Q1021635.jpg".into(),
    ];

    // Check that at least one test image exists
    let available: Vec<&PathBuf> = test_images.iter().filter(|p| p.exists()).collect();
    if available.is_empty() {
        eprintln!("No test images found in content/ - skipping comparison");
        return;
    }

    // Check ImageMagick is available
    if !magick_available() {
        eprintln!("ImageMagick not found - skipping comparison");
        return;
    }

    let _ = std::fs::remove_dir_all(OUTPUT_DIR);
    std::fs::create_dir_all(OUTPUT_DIR).unwrap();

    for source in &available {
        let stem = source.file_stem().unwrap().to_str().unwrap();
        println!("\n=== {} ===", stem);

        // Copy original
        std::fs::copy(source, format!("{}/{}_original.jpg", OUTPUT_DIR, stem)).unwrap();

        let (orig_w, orig_h) = image::image_dimensions(source).unwrap();
        let longer = orig_w.max(orig_h);
        println!("  Original: {}x{}", orig_w, orig_h);

        // Responsive sizes
        for &target in SIZES {
            if target > longer {
                println!("  Skipping {}w (exceeds original)", target);
                continue;
            }

            let (out_w, out_h) = if orig_w >= orig_h {
                let r = target as f64 / orig_w as f64;
                (target, (orig_h as f64 * r).round() as u32)
            } else {
                let r = target as f64 / orig_h as f64;
                ((orig_w as f64 * r).round() as u32, target)
            };

            // WebP
            let mw = PathBuf::from(format!("{}/{}_{}w_magick.webp", OUTPUT_DIR, stem, target));
            let rw = PathBuf::from(format!("{}/{}_{}w_rust.webp", OUTPUT_DIR, stem, target));

            let t = Instant::now();
            magick_resize(source, &mw, out_w, out_h, QUALITY);
            let m_ms = t.elapsed().as_millis();

            let t = Instant::now();
            rust_resize(source, &rw, out_w, out_h, QUALITY);
            let r_ms = t.elapsed().as_millis();

            println!(
                "  WebP  {}w: magick={}KB/{}ms  rust={}KB/{}ms",
                target,
                file_kb(&mw),
                m_ms,
                file_kb(&rw),
                r_ms
            );

            // AVIF
            let ma = PathBuf::from(format!("{}/{}_{}w_magick.avif", OUTPUT_DIR, stem, target));
            let ra = PathBuf::from(format!("{}/{}_{}w_rust.avif", OUTPUT_DIR, stem, target));

            let t = Instant::now();
            magick_resize(source, &ma, out_w, out_h, QUALITY);
            let m_ms = t.elapsed().as_millis();

            let t = Instant::now();
            rust_resize(source, &ra, out_w, out_h, QUALITY);
            let r_ms = t.elapsed().as_millis();

            println!(
                "  AVIF  {}w: magick={}KB/{}ms  rust={}KB/{}ms",
                target,
                file_kb(&ma),
                m_ms,
                file_kb(&ra),
                r_ms
            );
        }

        // Thumbnail
        let mt = PathBuf::from(format!("{}/{}_thumb_magick.webp", OUTPUT_DIR, stem));
        let rt = PathBuf::from(format!("{}/{}_thumb_rust.webp", OUTPUT_DIR, stem));

        let t = Instant::now();
        magick_thumbnail(source, &mt, THUMB_CROP_W, THUMB_CROP_H, QUALITY);
        let m_ms = t.elapsed().as_millis();

        let t = Instant::now();
        rust_thumbnail(source, &rt, THUMB_CROP_W, THUMB_CROP_H, QUALITY);
        let r_ms = t.elapsed().as_millis();

        println!(
            "  Thumb 400x500: magick={}KB/{}ms  rust={}KB/{}ms",
            file_kb(&mt),
            m_ms,
            file_kb(&rt),
            r_ms
        );
    }

    // List all generated files
    println!("\n--- Generated files in {} ---", OUTPUT_DIR);
    let mut entries: Vec<_> = std::fs::read_dir(OUTPUT_DIR)
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in &entries {
        let meta = entry.metadata().unwrap();
        println!(
            "  {:>8}KB  {}",
            meta.len() / 1024,
            entry.file_name().to_string_lossy()
        );
    }
    println!("\nTotal files: {}", entries.len());
    println!("Open in Finder: open {}", OUTPUT_DIR);
}
