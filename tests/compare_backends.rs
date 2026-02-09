//! Integration test for cross-backend dimension parity.
//!
//! Asserts that ImageMagick and Rust backends produce identical output
//! dimensions for resizes and thumbnails. Skips gracefully when
//! ImageMagick is not installed.
//!
//! For visual quality comparison, use the example instead:
//!   cargo run --example compare_backends

use image::{ImageEncoder, RgbImage};
use std::path::Path;
use std::process::Command;

const QUALITY: u32 = 90;

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

/// Automated parity test: synthetic images through both backends, assert dimensions match.
#[test]
fn cross_backend_dimension_parity() {
    if !magick_available() {
        eprintln!("ImageMagick not found - skipping cross-backend parity test");
        return;
    }

    let tmp = tempfile::TempDir::new().unwrap();

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
