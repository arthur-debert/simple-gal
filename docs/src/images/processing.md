# Processing Pipeline

Image processing is stage 2 of the Simple Gal build pipeline. It reads the manifest produced by the scan stage, processes every source image into responsive sizes and thumbnails, and writes the results to a temp directory for the generate stage to consume.

## What happens during processing

For each image in every album, the processing stage:

1. Reads the source file from your content directory
2. Extracts dimensions and any embedded IPTC metadata (title, description)
3. Generates AVIF files at each configured responsive size (skipping sizes larger than the source)
4. Generates a single AVIF thumbnail at the configured aspect ratio and size
5. Records all generated paths and dimensions in an output manifest

The output goes to `.simple-gal-temp/processed/`, organized by album:

```text
.simple-gal-temp/processed/
├── manifest.json
├── 010-Landscapes/
│   ├── 001-dawn-800.avif
│   ├── 001-dawn-1400.avif
│   ├── 001-dawn-2080.avif
│   ├── 001-dawn-thumb.avif
│   ├── 002-dusk-800.avif
│   ├── 002-dusk-1400.avif
│   ├── 002-dusk-2080.avif
│   └── 002-dusk-thumb.avif
└── 020-Portraits/
    ├── 001-studio-800.avif
    ├── 001-studio-1400.avif
    ├── 001-studio-2080.avif
    └── 001-studio-thumb.avif
```

The `manifest.json` contains the full metadata for every album and image, including generated file paths, dimensions, titles, descriptions, and resolved configuration. The generate stage reads this file to produce the final HTML site.

## Parallel processing

Simple Gal uses rayon to process images in parallel across CPU cores. By default, it uses all available cores. This makes a significant difference -- AVIF encoding is CPU-intensive, and a portfolio with 200 images can take minutes on a single core but seconds on a modern multi-core machine.

### Limiting threads

If you need to constrain CPU usage (for example, on a shared server or while doing other work), set `max_processes`:

```toml
[processing]
max_processes = 4
```

This caps the number of parallel workers. If you set a value higher than the number of available cores, it is clamped down to the core count. Omit the key entirely to use all cores.

Setting `max_processes = 1` disables parallelism and processes images sequentially.

## Input formats

Simple Gal accepts the following source image formats:

| Format | Extensions |
|--------|-----------|
| JPEG | `.jpg`, `.jpeg` |
| PNG | `.png` |
| TIFF | `.tiff`, `.tif` |
| WebP | `.webp` |

All input formats are converted to AVIF on output. The source files are never modified.

## Output format

Every generated file is AVIF, encoded with the rav1e encoder. This is a pure Rust AV1 implementation compiled into the Simple Gal binary. There are no system dependencies -- no ImageMagick, no FFmpeg, no shared libraries to install.

The resampling algorithm for all resizing is Lanczos3, which produces sharp results with minimal ringing artifacts.

## The temp directory

The `.simple-gal-temp/` directory is a build artifact. It holds the processed images and manifest between the process and generate stages. You can safely delete it at any time -- it will be recreated on the next build.

Add it to your `.gitignore`:

```text
.simple-gal-temp/
```

## Per-album configuration

Each album uses its own resolved configuration from the config chain. This means different albums can have different responsive sizes, quality settings, and thumbnail aspect ratios. The processing stage reads the per-album config from the scan manifest and applies it independently.

See [Responsive Sizes](responsive-sizes.md), [Thumbnails](thumbnails.md), and [Quality](quality.md) for the settings that control image output. See [Processing Cache](caching.md) for how repeated builds skip unchanged images.
