# Image Quality

Simple Gal encodes all output images -- responsive sizes and thumbnails -- in AVIF format. The `quality` setting controls the tradeoff between file size and visual fidelity.

## Configuration

```toml
[images]
quality = 90
```

The value is an integer from 0 to 100. Higher values produce larger files with fewer compression artifacts. Lower values produce smaller files with more visible degradation.

## Choosing a quality value

The default of 90 is a good starting point for fine art photography. At this level, compression artifacts are invisible at normal viewing distances, and the files are roughly half the size of equivalent JPEGs.

| Quality | Use case | Notes |
|---------|----------|-------|
| 95-100 | Archival, print-resolution work | Minimal compression. Large files. Diminishing returns above 95. |
| 85-90 | Portfolio display (default range) | Visually lossless for web viewing. Good balance. |
| 70-80 | Documentation, travel snapshots | Noticeable softening on close inspection. Significantly smaller files. |
| Below 70 | Not recommended | Visible artifacts, especially in gradients and fine detail. |

For most photography portfolios, values between 85 and 90 are the sweet spot. Going above 90 increases file size substantially with no perceptible improvement on screen.

## AVIF vs JPEG

AVIF achieves better compression than JPEG at the same visual quality. As a rough guide:

- AVIF quality 90 is comparable to JPEG quality 95 in perceived sharpness
- AVIF files at quality 90 are typically 40-60% smaller than the equivalent JPEG

Simple Gal uses the rav1e encoder, a pure Rust AV1 implementation. This means no system dependencies -- the encoder is built into the binary. The tradeoff is that encoding is slower than hardware-accelerated alternatives, but this is offset by parallel processing across CPU cores.

## Per-gallery overrides

Quality can be overridden at any level of the config chain. A gallery of phone snapshots might use a lower quality to reduce output size:

```toml
# content/030-Snapshots/config.toml
[images]
quality = 75
```

This gallery uses quality 75 while other galleries inherit the root config's quality 90. The quality setting applies to both responsive images and thumbnails for that gallery.

## File size impact

To give a sense of scale, here are approximate file sizes for a single 2080px-wide landscape image at different quality levels:

| Quality | Approximate size |
|---------|-----------------|
| 100 | 800-1200 KB |
| 90 | 200-400 KB |
| 80 | 100-200 KB |
| 70 | 60-120 KB |

Actual sizes vary with image content. Images with smooth gradients (skies, studio backdrops) compress better than images with fine detail (foliage, textured surfaces).
