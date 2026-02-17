# Responsive Sizes

Visitors view your portfolio on screens ranging from phones to 5K displays. Serving a single image size wastes bandwidth on small screens and looks soft on large ones. Simple Gal solves this by generating multiple sizes of each image and letting the browser pick the best one.

## How it works

For each source image, Simple Gal produces an AVIF file at every configured breakpoint width. The generated HTML uses a standard `<img>` tag with a `srcset` attribute:

```html
<img src="001-dawn-1400.avif"
     srcset="001-dawn-800.avif 800w,
             001-dawn-1400.avif 1400w,
             001-dawn-2080.avif 2080w"
     sizes="(max-width: 800px) 100vw, 80vw"
     alt="Dawn">
```

The browser reads the `srcset` list, considers the viewport width and device pixel ratio, and downloads only the size it needs. A phone on a cellular connection gets the 800px version; a retina desktop gets the 2080px version. You do nothing at runtime -- the browser handles selection automatically.

## Configuring sizes

Set the breakpoints in your `config.toml`:

```toml
[images]
sizes = [800, 1400, 2080]
```

Each value is a pixel width for the **longer edge** of the image. Simple Gal preserves the original aspect ratio and calculates the shorter edge proportionally.

For example, a 4000x3000 landscape source at size 800 produces an 800x600 AVIF. A 3000x4000 portrait source at size 800 produces an 600x800 AVIF.

### Choosing breakpoints

The defaults cover most use cases:

| Size | Target |
|------|--------|
| 800  | Phones, small tablets |
| 1400 | Laptops, standard desktops |
| 2080 | Large/retina displays |

If your audience skews toward high-end monitors, add a larger size:

```toml
[images]
sizes = [800, 1400, 2080, 3200]
```

If you want faster builds and smaller output at the cost of sharpness on large screens, trim to two sizes:

```toml
[images]
sizes = [800, 1600]
```

More sizes mean more files and longer processing time, but each additional size only affects images large enough to benefit from it.

## Small source images

When a source image is smaller than a configured size, that size is skipped. Simple Gal never upscales.

Consider `sizes = [800, 1400, 2080]` with a 1200x900 source:

- **800**: generated (source is large enough)
- **1400**: skipped (source is only 1200px wide)
- **2080**: skipped

If the source is smaller than every configured size, Simple Gal generates a single AVIF at the original dimensions. The image is still converted to AVIF for the file size benefit, but no scaling occurs.

## Output format

All responsive images are encoded as AVIF. There is no option to output JPEG or WebP -- AVIF provides better compression at equivalent visual quality, and browser support is broad enough for a photography portfolio.

The encoding quality is controlled separately. See [Quality](quality.md) for details.

## Per-gallery overrides

Responsive sizes are set at any level of the config chain. A travel album with low-resolution phone photos might use smaller breakpoints:

```toml
# content/020-Travel/010-Phone-Snaps/config.toml
[images]
sizes = [400, 800]
```

This album generates only two sizes while other albums use the root config's three sizes. See [Configuration Overview](../configuration/overview.md) for how the config chain works.
