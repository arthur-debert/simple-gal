# Thumbnails

Thumbnails are the small preview images shown in album grids. Every image gets a thumbnail that is cropped to a consistent aspect ratio so the grid looks clean regardless of the mix of portrait and landscape originals.

## How thumbnails are created

The process has two steps:

1. **Resize to fill** -- the source image is scaled down using Lanczos3 resampling so that it completely covers the target dimensions, with no empty space.
2. **Center crop** -- any overflow is trimmed equally from both sides, keeping the center of the image.

This is the same "cover" behavior you see in CSS `object-fit: cover`. A landscape photo cropped to a portrait thumbnail loses the left and right edges; a portrait photo cropped to a landscape thumbnail loses the top and bottom.

After cropping, a light unsharp mask (sigma 0.5, threshold 0) is applied to keep thumbnails crisp at small sizes.

## Configuration

Two settings control thumbnail geometry:

```toml
[thumbnails]
aspect_ratio = [4, 5]
size = 400
```

### Aspect ratio

`aspect_ratio` is expressed as `[width, height]`. The first value is the horizontal proportion; the second is the vertical.

```toml
# Portrait (taller than wide)
aspect_ratio = [4, 5]

# Square
aspect_ratio = [1, 1]

# Landscape (wider than tall)
aspect_ratio = [3, 2]
```

Common choices:

| Ratio | Shape | Notes |
|-------|-------|-------|
| `[4, 5]` | Portrait | Default. Works well for figure photography and vertical compositions. |
| `[1, 1]` | Square | Clean, symmetric grids. Good all-rounder. |
| `[3, 2]` | Landscape | Matches the 35mm frame. Good for horizontal work. |
| `[16, 9]` | Wide landscape | Cinematic feel, but crops aggressively on portrait originals. |
| `[4, 3]` | Mild landscape | Less aggressive crop than 16:9. |

### Size

`size` is the **short edge** of the thumbnail in pixels. The long edge is calculated from the aspect ratio.

With `aspect_ratio = [4, 5]` and `size = 400`:

- Short edge (width) = 400px
- Long edge (height) = 400 * 5/4 = 500px
- Final thumbnail: 400x500 pixels

With `aspect_ratio = [3, 2]` and `size = 300`:

- Short edge (height) = 300px
- Long edge (width) = 300 * 3/2 = 450px
- Final thumbnail: 450x300 pixels

The default of 400px produces sharp thumbnails on standard and retina screens without excessive file sizes.

## Per-gallery overrides

Each album can override thumbnail settings through its own `config.toml`. This is useful when a gallery has a different visual character.

```toml
# content/010-Landscapes/config.toml
[thumbnails]
aspect_ratio = [3, 2]
```

This album gets landscape thumbnails while every other album uses the root config's portrait ratio.

You can also override at the group level. All albums under a group inherit the group's settings:

```toml
# content/020-Travel/config.toml
[thumbnails]
aspect_ratio = [1, 1]
size = 350
```

Now every album under `020-Travel` uses square 350px thumbnails, unless an individual album overrides again.

Only the keys you specify are overridden. Setting `aspect_ratio` in a gallery config does not reset `size` to the default -- it keeps whatever value was inherited from the parent.

## Custom album thumbnail

By default, the album thumbnail is image #1 (or the first image by sort order). To designate a specific image as the album thumbnail, use the `thumb` naming convention:

```text
content/010-Landscapes/
├── 001-dawn.jpg
├── 002-dusk.jpg
├── 005-thumb.jpg           # ← This image becomes the album thumbnail
└── 010-night.jpg
```

The thumb image is **not** included in the gallery — it is only used as the album's representative thumbnail on the index page. It does not appear as a browsable photo in the album.

### Naming rules

Any image whose name (after the number prefix) starts with `thumb` is a thumb designator:

| Filename | Thumb? |
|----------|--------|
| `005-thumb.jpg` | Yes |
| `005-thumb-The-Sunset.jpg` | Yes |
| `thumb.jpg` | Yes |
| `001-thumbnail.jpg` | No |

Only one thumb image is allowed per album. If two or more images match, the build fails with a `DuplicateThumb` error.

### Priority

1. Thumb-designated image (if present)
2. Image #1 (number prefix `001`)
3. First image by sort order

## Output format

Thumbnails are encoded as AVIF using the same quality setting as responsive images. Each thumbnail is saved as `{stem}-thumb.avif` alongside the responsive sizes:

```text
processed/010-Landscapes/
├── 001-dawn-800.avif
├── 001-dawn-1400.avif
├── 001-dawn-2080.avif
└── 001-dawn-thumb.avif
```
