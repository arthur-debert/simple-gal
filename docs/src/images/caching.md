# Processing Cache

AVIF encoding is the slowest part of the build pipeline. A portfolio with dozens of images can take minutes to process from scratch. The processing cache makes repeated builds near-instant by skipping encoding when the source image and encoding parameters haven't changed.

## How it works

On each build, Simple Gal computes a SHA-256 hash of every source image and of the encoding parameters (size, quality, aspect ratio, sharpening) used for each output file. If a previous build already produced the same output from the same source with the same parameters, and the output file still exists on disk, the encoding step is skipped entirely.

Everything else always runs: scanning the filesystem, reading IPTC metadata, resolving titles and descriptions, computing dimensions. This means metadata changes (e.g. updating a title in Lightroom) are picked up immediately without any cache busting.

## What you see

The build output shows cache statistics after the processing stage:

```text
==> Stage 2: Processing images
001 Landscapes (5 photos)
    Dawn → 2080 1400 800 + thumb
    Dusk → 2080 1400 800 + thumb
Processed 1 albums, 5 images
Cache: 20 cached, 0 encoded (20 total)
```

On a cold build (first run or after `--no-cache`):

```text
Cache: 20 encoded
```

## Cache location

The cache manifest is stored at `.simple-gal-temp/processed/.cache-manifest.json` alongside the processed images. Deleting the temp directory clears the cache.

## Bypassing the cache

To force a full rebuild, pass `--no-cache`:

```bash
simple-gal build --no-cache
```

This re-encodes every image regardless of whether the cache would hit. Use this after upgrading Simple Gal if you want to pick up encoder improvements, or if you suspect cache corruption.

## What invalidates the cache

Each output file is individually tracked. The cache is invalidated when:

- **Source image changes**: editing, replacing, or re-exporting the source file
- **Encoding parameters change**: modifying `sizes`, `quality`, or `thumbnails` in `config.toml`
- **Output file is deleted**: if someone removes processed files manually

Adding or removing images from an album does not invalidate the cache for other images in the same album.

## CI and GitHub Actions

The cache works naturally in CI if the processed output directory persists between runs. With GitHub Actions, use `actions/cache` to cache `.simple-gal-temp/processed/`:

```yaml
- name: Cache processed images
  uses: actions/cache@v4
  with:
    path: .simple-gal-temp/processed
    key: processed-${{ hashFiles('content/**') }}
    restore-keys: processed-

- name: Build gallery
  uses: arthur-debert/simple-gal-action@v1
```

On subsequent pushes, only new or changed images are re-encoded. The rest are served from cache.
