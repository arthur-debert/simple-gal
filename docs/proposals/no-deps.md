# Proposal: Zero-Dependency Binary (Eliminate ImageMagick)

## Goal

Replace all ImageMagick shell-outs with Rust crate equivalents, compiled statically into
the binary. The result: a single binary with **zero runtime dependencies** that can run
decades from now on any system with a compatible CPU and a browser.

## Current State

Binary size: **2.2 MB** (release, stripped).

ImageMagick is invoked via `std::process::Command` in `src/imaging/backend.rs`, behind
the `ImageBackend` trait. Five operations:

| # | Command | Purpose |
|---|---------|---------|
| 1 | `identify -format "%w %h"` | Get image dimensions |
| 2 | `identify -format "%[IPTC:2:05]\t%[IPTC:2:120]"` | Read IPTC title + caption |
| 3 | `convert -resize WxH -quality Q dst.webp` | Resize to WebP |
| 4 | `convert -resize WxH -quality Q -define heic:speed=6 dst.avif` | Resize to AVIF |
| 5 | `convert -resize WxH^ -gravity center -extent WxH -quality Q [-sharpen RxS] dst` | Thumbnail (fill + center-crop + sharpen) |

Input formats: JPG, PNG, WebP. Output formats: AVIF, WebP.

## Proposed Solution

Use the `image` crate ecosystem:

```toml
image = { version = "0.25", default-features = false, features = [
    "jpeg", "png", "webp",   # decoding
    "webp-encoder",           # lossy WebP encoding (vendored libwebp)
    "avif",                   # AVIF encoding (ravif/rav1e, ~pure Rust)
] }
iptc = "0.1"                  # IPTC metadata reading (pure Rust, JPEG only)
```

### Operation Mapping

| Current (ImageMagick) | Replacement | Pure Rust? |
|------------------------|-------------|------------|
| `identify` (dimensions) | `image::image_dimensions()` or decode headers | Yes |
| `identify` (IPTC) | `iptc` crate | Yes (JPEG only) |
| `convert -resize` | `image::imageops::resize(..., Lanczos3)` | Yes |
| WebP encode (lossy) | `webp-encoder` feature | Vendored C (libwebp-sys), zero system deps |
| AVIF encode | `avif` feature (ravif/rav1e) | ~Pure Rust |
| Thumbnail (fill+crop) | `resize_to_fill()` + crop | Yes |
| Sharpen | `image::imageops::unsharpen()` | Yes |

### Why Not Other Approaches?

**magick-rust** (Rust bindings to libMagickWand): Requires ImageMagick 7.x installed.
Static linking poorly supported. Trades shell-out problems for FFI binding problems
while keeping the same fundamental dependency. Defeats the purpose.

**libvips bindings**: Requires libvips + glib + dozens of transitive C libraries.
Static linking impractical due to LGPL dependencies (glib, etc.). Same problem.

## Binary Size Impact

| Component | Size |
|-----------|------|
| Current binary | 2.2 MB |
| `image` core (decode + resize + crop + sharpen) | +~400 KB |
| `libwebp-sys` (lossy WebP encode) | +~300 KB |
| `rav1e` + `ravif` (AVIF encode) | +~2-4 MB |
| `iptc` | negligible |
| **Estimated total** | **~5-7 MB** |

~3x current size. Still a very reasonable single binary.

## Quality Assessment

- **Resize (Lanczos3)**: Visually comparable to ImageMagick's Lanczos for photo downsizing.
- **WebP lossy**: Identical -- same underlying libwebp library.
- **AVIF**: Comparable quality at similar settings. `rav1e` at speed 6 matches ImageMagick.
- **Sharpen**: Parameter mapping differs. ImageMagick `-sharpen 0x0.5` maps approximately
  to `unsharpen(img, 0.5, 0)`. Needs visual tuning.

## Known Gotchas

1. **IPTC on non-JPEG**: The `iptc` crate only supports JPEG. Photographer source files
   are virtually always JPEG, so this is acceptable. PNG/WebP rarely carry IPTC in practice.
2. **AVIF encoding speed**: `rav1e` is slower than ImageMagick's `libaom` delegate.
   Parallelization with rayon (already in deps) compensates.
3. **rav1e compile time**: Adds 2-5 minutes to cold builds. Incremental builds are fast.
4. **Sharpen parameter tuning**: Not 1:1 with ImageMagick. Visual comparison needed.
5. **WebP encoder is vendored C**: Not pure Rust, but compiles from source automatically
   with no system library required. Needs a C compiler at build time (standard in CI).

## Implementation Plan

1. Add `image` and `iptc` dependencies to `Cargo.toml`.
2. Implement `RustBackend` for the existing `ImageBackend` trait.
3. Add unit tests for `RustBackend`.
4. Generate visual comparison triads (original / ImageMagick / Rust) for quality review.
5. If quality is acceptable, wire `RustBackend` as the default (remove `ImageMagickBackend`).
6. Update README to remove the ImageMagick prerequisite.

## Verification

This proposal includes a visual comparison step: for sample images from `content/`,
generate outputs at all configured sizes using both backends side-by-side in `/tmp/foo/`,
allowing direct visual inspection before committing to the migration.
