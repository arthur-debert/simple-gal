//! Pure Rust image processing backend — zero external dependencies.
//!
//! Everything is statically linked into the binary.
//!
//! ## Crate mapping
//!
//! | Operation | Crate / function |
//! |---|---|
//! | Decode (JPEG, PNG, TIFF, WebP) | `image` crate (pure Rust decoders) |
//! | Decode (AVIF) | `avif-parse` (container) + `rav1d` (AV1 decode) + custom YUV→RGB |
//! | Resize | `image::imageops::resize` with `Lanczos3` filter |
//! | Encode → AVIF | `image::codecs::avif::AvifEncoder` (rav1e, speed 6) |
//! | Thumbnail crop | `image::DynamicImage::resize_to_fill` |
//! | Sharpening | `image::imageops::unsharpen` |
//! | IPTC metadata | custom `iptc_parser` (JPEG APP13 + TIFF IFD) |

use super::backend::{BackendError, Dimensions, ImageBackend, ImageMetadata};
use super::params::{ResizeParams, ThumbnailParams};
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat, ImageReader};
use std::path::Path;
use std::sync::LazyLock;

/// Extensions whose decoders are compiled in and known to work.
///
/// AVIF is deliberately excluded: the `image` crate's `"avif"` feature only enables the
/// **encoder** (rav1e). The decoder requires `"avif-native"` (a C library we don't use).
/// `ImageFormat::reading_enabled()` incorrectly returns `true` for AVIF when `"avif"` is
/// enabled, so we cannot rely on that API alone.
const PHOTO_CANDIDATES: &[(&str, ImageFormat)] = &[
    ("jpg", ImageFormat::Jpeg),
    ("jpeg", ImageFormat::Jpeg),
    ("png", ImageFormat::Png),
    ("tif", ImageFormat::Tiff),
    ("tiff", ImageFormat::Tiff),
    ("webp", ImageFormat::WebP),
];

static SUPPORTED_EXTENSIONS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    let mut exts: Vec<&'static str> = PHOTO_CANDIDATES
        .iter()
        .filter(|(_, fmt)| fmt.reading_enabled())
        .map(|(ext, _)| *ext)
        .collect();
    // AVIF is decoded via our custom rav1d-based decoder (not the image crate)
    exts.push("avif");
    exts
});

/// Returns the set of image file extensions that have working decoders compiled in.
pub fn supported_input_extensions() -> &'static [&'static str] {
    &SUPPORTED_EXTENSIONS
}

/// Pure Rust backend using the `image` crate ecosystem.
///
/// See the [module docs](self) for the crate-to-operation mapping.
pub struct RustBackend;

impl RustBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RustBackend {
    fn default() -> Self {
        Self::new()
    }
}

fn is_avif(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("avif"))
}

/// Load and decode an image from disk.
fn load_image(path: &Path) -> Result<DynamicImage, BackendError> {
    if is_avif(path) {
        return decode_avif(path);
    }
    ImageReader::open(path)
        .map_err(BackendError::Io)?
        .decode()
        .map_err(|e| {
            BackendError::ProcessingFailed(format!("Failed to decode {}: {}", path.display(), e))
        })
}

/// Extract dimensions from an AVIF file's container metadata (no full decode needed).
fn identify_avif(path: &Path) -> Result<Dimensions, BackendError> {
    let file_data = std::fs::read(path).map_err(BackendError::Io)?;
    let avif = avif_parse::read_avif(&mut std::io::Cursor::new(&file_data)).map_err(|e| {
        BackendError::ProcessingFailed(format!("Failed to parse AVIF {}: {e:?}", path.display()))
    })?;
    let meta = avif.primary_item_metadata().map_err(|e| {
        BackendError::ProcessingFailed(format!(
            "Failed to read AVIF metadata {}: {e:?}",
            path.display()
        ))
    })?;
    Ok(Dimensions {
        width: meta.max_frame_width.get(),
        height: meta.max_frame_height.get(),
    })
}

/// Decode an AVIF file using avif-parse (container) + rav1d (AV1 decode).
///
/// The `image` crate's `"avif"` feature only provides the encoder (rav1e).
/// Decoding requires `"avif-native"` which depends on the C library dav1d.
/// Instead, we use `rav1d` (pure Rust port of dav1d) directly.
fn decode_avif(path: &Path) -> Result<DynamicImage, BackendError> {
    use rav1d::include::dav1d::data::Dav1dData;
    use rav1d::include::dav1d::dav1d::Dav1dSettings;
    use rav1d::include::dav1d::headers::{
        DAV1D_PIXEL_LAYOUT_I400, DAV1D_PIXEL_LAYOUT_I420, DAV1D_PIXEL_LAYOUT_I422,
        DAV1D_PIXEL_LAYOUT_I444,
    };
    use rav1d::include::dav1d::picture::Dav1dPicture;
    use std::ptr::NonNull;

    let file_data = std::fs::read(path).map_err(BackendError::Io)?;
    let avif = avif_parse::read_avif(&mut std::io::Cursor::new(&file_data)).map_err(|e| {
        BackendError::ProcessingFailed(format!("Failed to parse AVIF {}: {e:?}", path.display()))
    })?;
    let av1_bytes: &[u8] = &avif.primary_item;

    // Initialize rav1d decoder
    let mut settings = std::mem::MaybeUninit::<Dav1dSettings>::uninit();
    unsafe {
        rav1d::src::lib::dav1d_default_settings(NonNull::new(settings.as_mut_ptr()).unwrap())
    };
    let mut settings = unsafe { settings.assume_init() };
    settings.n_threads = 1;
    settings.max_frame_delay = 1;

    let mut ctx = None;
    let rc =
        unsafe { rav1d::src::lib::dav1d_open(NonNull::new(&mut ctx), NonNull::new(&mut settings)) };
    if rc.0 != 0 {
        return Err(BackendError::ProcessingFailed(format!(
            "rav1d open failed ({})",
            rc.0
        )));
    }

    // Create data buffer and copy AV1 bytes
    let mut data = Dav1dData::default();
    let buf_ptr =
        unsafe { rav1d::src::lib::dav1d_data_create(NonNull::new(&mut data), av1_bytes.len()) };
    if buf_ptr.is_null() {
        unsafe { rav1d::src::lib::dav1d_close(NonNull::new(&mut ctx)) };
        return Err(BackendError::ProcessingFailed(
            "rav1d data_create failed".into(),
        ));
    }
    unsafe { std::ptr::copy_nonoverlapping(av1_bytes.as_ptr(), buf_ptr, av1_bytes.len()) };

    // Feed data to decoder
    let rc = unsafe { rav1d::src::lib::dav1d_send_data(ctx, NonNull::new(&mut data)) };
    if rc.0 != 0 {
        unsafe {
            rav1d::src::lib::dav1d_data_unref(NonNull::new(&mut data));
            rav1d::src::lib::dav1d_close(NonNull::new(&mut ctx));
        }
        return Err(BackendError::ProcessingFailed(format!(
            "rav1d send_data failed ({})",
            rc.0
        )));
    }

    // Get decoded picture
    let mut pic: Dav1dPicture = unsafe { std::mem::zeroed() };
    let rc = unsafe { rav1d::src::lib::dav1d_get_picture(ctx, NonNull::new(&mut pic)) };
    if rc.0 != 0 {
        unsafe { rav1d::src::lib::dav1d_close(NonNull::new(&mut ctx)) };
        return Err(BackendError::ProcessingFailed(format!(
            "rav1d get_picture failed ({})",
            rc.0
        )));
    }

    // Extract dimensions and pixel layout
    let w = pic.p.w as u32;
    let h = pic.p.h as u32;
    let bpc = pic.p.bpc as u32;
    let layout = pic.p.layout;
    let y_stride = pic.stride[0];
    let uv_stride = pic.stride[1];
    let y_ptr = pic.data[0].unwrap().as_ptr() as *const u8;

    // Convert YUV planes to interleaved RGB8
    let rgb = if layout == DAV1D_PIXEL_LAYOUT_I400 {
        YuvPlanes {
            y_ptr,
            u_ptr: y_ptr,
            v_ptr: y_ptr,
            y_stride,
            uv_stride: 0,
            width: w,
            height: h,
            bpc,
            ss_x: false,
            ss_y: false,
            monochrome: true,
        }
        .to_rgb()
    } else {
        let u_ptr = pic.data[1].unwrap().as_ptr() as *const u8;
        let v_ptr = pic.data[2].unwrap().as_ptr() as *const u8;
        let (ss_x, ss_y) = match layout {
            DAV1D_PIXEL_LAYOUT_I420 => (true, true),
            DAV1D_PIXEL_LAYOUT_I422 => (true, false),
            DAV1D_PIXEL_LAYOUT_I444 => (false, false),
            _ => {
                unsafe {
                    rav1d::src::lib::dav1d_picture_unref(NonNull::new(&mut pic));
                    rav1d::src::lib::dav1d_close(NonNull::new(&mut ctx));
                }
                return Err(BackendError::ProcessingFailed(format!(
                    "Unsupported AVIF pixel layout: {layout}"
                )));
            }
        };
        YuvPlanes {
            y_ptr,
            u_ptr,
            v_ptr,
            y_stride,
            uv_stride,
            width: w,
            height: h,
            bpc,
            ss_x,
            ss_y,
            monochrome: false,
        }
        .to_rgb()
    };

    unsafe {
        rav1d::src::lib::dav1d_picture_unref(NonNull::new(&mut pic));
        rav1d::src::lib::dav1d_close(NonNull::new(&mut ctx));
    }

    image::RgbImage::from_raw(w, h, rgb)
        .map(DynamicImage::ImageRgb8)
        .ok_or_else(|| {
            BackendError::ProcessingFailed("Failed to create image from decoded AVIF data".into())
        })
}

/// Decoded YUV plane data from rav1d, ready for RGB conversion.
struct YuvPlanes {
    y_ptr: *const u8,
    u_ptr: *const u8,
    v_ptr: *const u8,
    y_stride: isize,
    uv_stride: isize,
    width: u32,
    height: u32,
    bpc: u32,
    /// Chroma subsampling: horizontal, vertical (e.g. I420 = true, true)
    ss_x: bool,
    ss_y: bool,
    monochrome: bool,
}

impl YuvPlanes {
    /// Convert YUV planes to interleaved RGB8 using BT.601 coefficients.
    fn to_rgb(&self) -> Vec<u8> {
        let max_val = ((1u32 << self.bpc) - 1) as f32;
        let center = (1u32 << (self.bpc - 1)) as f32;
        let scale = 255.0 / max_val;

        let mut rgb = vec![0u8; (self.width * self.height * 3) as usize];

        for row in 0..self.height {
            for col in 0..self.width {
                let y_val = read_pixel(self.y_ptr, self.y_stride, col, row, self.bpc);

                let (r, g, b) = if self.monochrome {
                    let v = (y_val * scale).clamp(0.0, 255.0);
                    (v, v, v)
                } else {
                    let u_col = if self.ss_x { col / 2 } else { col };
                    let u_row = if self.ss_y { row / 2 } else { row };
                    let cb = read_pixel(self.u_ptr, self.uv_stride, u_col, u_row, self.bpc);
                    let cr = read_pixel(self.v_ptr, self.uv_stride, u_col, u_row, self.bpc);

                    // BT.601 YCbCr -> RGB, then scale to 8-bit
                    let cb_f = cb - center;
                    let cr_f = cr - center;

                    (
                        ((y_val + 1.402 * cr_f) * scale).clamp(0.0, 255.0),
                        ((y_val - 0.344136 * cb_f - 0.714136 * cr_f) * scale).clamp(0.0, 255.0),
                        ((y_val + 1.772 * cb_f) * scale).clamp(0.0, 255.0),
                    )
                };

                let idx = ((row * self.width + col) * 3) as usize;
                rgb[idx] = r as u8;
                rgb[idx + 1] = g as u8;
                rgb[idx + 2] = b as u8;
            }
        }

        rgb
    }
}

/// Read a single pixel value from a YUV plane, handling both 8-bit and 16-bit storage.
#[inline]
fn read_pixel(ptr: *const u8, stride: isize, x: u32, y: u32, bpc: u32) -> f32 {
    if bpc <= 8 {
        (unsafe { *ptr.offset(y as isize * stride + x as isize) }) as f32
    } else {
        // 10-bit and 12-bit are stored as u16
        let byte_offset = y as isize * stride + x as isize * 2;
        (unsafe { *(ptr.offset(byte_offset) as *const u16) }) as f32
    }
}

/// Save a DynamicImage to the given path, inferring format from extension.
fn save_image(img: &DynamicImage, path: &Path, quality: u32) -> Result<(), BackendError> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "avif" => save_avif(img, path, quality),
        other => Err(BackendError::ProcessingFailed(format!(
            "Unsupported output format: {}",
            other
        ))),
    }
}

/// Encode and save as AVIF using ravif/rav1e (speed=6 for reasonable throughput).
fn save_avif(img: &DynamicImage, path: &Path, quality: u32) -> Result<(), BackendError> {
    let file = std::fs::File::create(path).map_err(BackendError::Io)?;
    let writer = std::io::BufWriter::new(file);
    let encoder =
        image::codecs::avif::AvifEncoder::new_with_speed_quality(writer, 6, quality as u8);
    img.write_with_encoder(encoder)
        .map_err(|e| BackendError::ProcessingFailed(format!("AVIF encode failed: {}", e)))
}

impl ImageBackend for RustBackend {
    fn identify(&self, path: &Path) -> Result<Dimensions, BackendError> {
        if is_avif(path) {
            return identify_avif(path);
        }
        let (width, height) = image::image_dimensions(path).map_err(|e| {
            BackendError::ProcessingFailed(format!("Failed to read dimensions: {}", e))
        })?;
        Ok(Dimensions { width, height })
    }

    fn read_metadata(&self, path: &Path) -> Result<ImageMetadata, BackendError> {
        let iptc = super::iptc_parser::read_iptc(path);
        Ok(ImageMetadata {
            title: iptc.object_name,
            description: iptc.caption,
            keywords: iptc.keywords,
        })
    }

    fn resize(&self, params: &ResizeParams) -> Result<(), BackendError> {
        let img = load_image(&params.source)?;
        let resized = img.resize(params.width, params.height, FilterType::Lanczos3);
        save_image(&resized, &params.output, params.quality.value())
    }

    fn thumbnail(&self, params: &ThumbnailParams) -> Result<(), BackendError> {
        let img = load_image(&params.source)?;

        // Fill-resize then center-crop to exact dimensions
        let filled =
            img.resize_to_fill(params.crop_width, params.crop_height, FilterType::Lanczos3);

        // Apply sharpening if requested
        let final_img = if let Some(sharpening) = params.sharpening {
            DynamicImage::from(image::imageops::unsharpen(
                &filled,
                sharpening.sigma,
                sharpening.threshold,
            ))
        } else {
            filled
        };

        save_image(&final_img, &params.output, params.quality.value())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imaging::params::{Quality, Sharpening};
    use image::{ImageEncoder, RgbImage};

    #[test]
    fn supported_extensions_match_decodable_formats() {
        let exts = super::supported_input_extensions();
        for expected in &["jpg", "jpeg", "png", "tif", "tiff", "webp", "avif"] {
            assert!(
                exts.contains(expected),
                "expected {expected} in supported extensions"
            );
        }
    }

    /// Create a small valid JPEG file with the given dimensions.
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

    #[test]
    fn identify_synthetic_jpeg() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("test.jpg");
        create_test_jpeg(&path, 200, 150);

        let backend = RustBackend::new();
        let dims = backend.identify(&path).unwrap();
        assert_eq!(dims.width, 200);
        assert_eq!(dims.height, 150);
    }

    #[test]
    fn identify_nonexistent_file_errors() {
        let backend = RustBackend::new();
        let result = backend.identify(Path::new("/nonexistent/image.jpg"));
        assert!(result.is_err());
    }

    #[test]
    fn read_metadata_synthetic_returns_default() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("test.jpg");
        create_test_jpeg(&path, 100, 100);

        let backend = RustBackend::new();
        let meta = backend.read_metadata(&path).unwrap();
        assert_eq!(meta, ImageMetadata::default());
    }

    #[test]
    fn read_metadata_nonexistent_returns_default() {
        let backend = RustBackend::new();
        let meta = backend
            .read_metadata(Path::new("/nonexistent/image.jpg"))
            .unwrap();
        assert_eq!(meta, ImageMetadata::default());
    }

    #[test]
    fn resize_synthetic_to_avif() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("source.jpg");
        create_test_jpeg(&source, 400, 300);

        let output = tmp.path().join("resized.avif");
        let backend = RustBackend::new();
        backend
            .resize(&ResizeParams {
                source,
                output: output.clone(),
                width: 200,
                height: 150,
                quality: Quality::new(85),
            })
            .unwrap();

        assert!(output.exists());
        assert!(std::fs::metadata(&output).unwrap().len() > 0);
    }

    #[test]
    fn resize_unsupported_format_errors() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("source.jpg");
        create_test_jpeg(&source, 100, 100);

        let output = tmp.path().join("output.webp");
        let backend = RustBackend::new();
        let result = backend.resize(&ResizeParams {
            source,
            output,
            width: 50,
            height: 50,
            quality: Quality::new(85),
        });
        assert!(result.is_err());
    }

    #[test]
    fn thumbnail_synthetic_exact_dimensions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("source.jpg");
        create_test_jpeg(&source, 800, 600);

        let output = tmp.path().join("thumb.avif");
        let backend = RustBackend::new();
        backend
            .thumbnail(&ThumbnailParams {
                source,
                output: output.clone(),
                crop_width: 400,
                crop_height: 500,
                quality: Quality::new(85),
                sharpening: Some(Sharpening::light()),
            })
            .unwrap();

        assert!(output.exists());
        assert!(std::fs::metadata(&output).unwrap().len() > 0);
    }

    #[test]
    fn thumbnail_synthetic_portrait_source() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("source.jpg");
        create_test_jpeg(&source, 600, 800);

        let output = tmp.path().join("thumb.avif");
        let backend = RustBackend::new();
        backend
            .thumbnail(&ThumbnailParams {
                source,
                output: output.clone(),
                crop_width: 400,
                crop_height: 500,
                quality: Quality::new(85),
                sharpening: Some(Sharpening::light()),
            })
            .unwrap();

        assert!(output.exists());
        assert!(std::fs::metadata(&output).unwrap().len() > 0);
    }

    #[test]
    fn thumbnail_synthetic_without_sharpening() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("source.jpg");
        create_test_jpeg(&source, 400, 300);

        let output = tmp.path().join("thumb.avif");
        let backend = RustBackend::new();
        backend
            .thumbnail(&ThumbnailParams {
                source,
                output: output.clone(),
                crop_width: 200,
                crop_height: 200,
                quality: Quality::new(85),
                sharpening: None,
            })
            .unwrap();

        assert!(output.exists());
        assert!(std::fs::metadata(&output).unwrap().len() > 0);
    }

    /// Create a small valid AVIF file by encoding a JPEG through our AVIF encoder.
    fn create_test_avif(path: &Path, width: u32, height: u32) {
        let img = RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        });
        let dynamic = DynamicImage::ImageRgb8(img);
        super::save_avif(&dynamic, path, 85).unwrap();
    }

    #[test]
    fn decode_avif_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let avif_path = tmp.path().join("test.avif");
        create_test_avif(&avif_path, 64, 48);

        let decoded = super::decode_avif(&avif_path).unwrap();
        assert_eq!(decoded.width(), 64);
        assert_eq!(decoded.height(), 48);
    }

    #[test]
    fn identify_avif_dimensions() {
        let tmp = tempfile::TempDir::new().unwrap();
        let avif_path = tmp.path().join("test.avif");
        create_test_avif(&avif_path, 120, 80);

        let dims = super::identify_avif(&avif_path).unwrap();
        assert_eq!(dims.width, 120);
        assert_eq!(dims.height, 80);
    }

    #[test]
    fn resize_avif_input_to_avif_output() {
        let tmp = tempfile::TempDir::new().unwrap();
        let source = tmp.path().join("source.avif");
        create_test_avif(&source, 200, 150);

        let output = tmp.path().join("resized.avif");
        let backend = RustBackend::new();
        backend
            .resize(&ResizeParams {
                source,
                output: output.clone(),
                width: 100,
                height: 75,
                quality: Quality::new(85),
            })
            .unwrap();

        assert!(output.exists());
        assert!(std::fs::metadata(&output).unwrap().len() > 0);
    }
}
