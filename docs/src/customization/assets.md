# Assets Directory

The `assets/` directory holds static files -- favicons, fonts, images, and anything else you want served alongside your portfolio. Its entire contents are copied verbatim to the output root during the build.

## Basic Usage

Place an `assets/` directory next to your `config.toml`:

```
my-portfolio/
  config.toml
  assets/
    favicon.svg
    fonts/
      my-font.woff2
    custom.css
  01-landscapes/
    ...
```

After building, these files appear at the output root:

```
dist/
  favicon.svg          # → accessible at /favicon.svg
  fonts/
    my-font.woff2      # → accessible at /fonts/my-font.woff2
  custom.css           # → accessible at /custom.css
  ...
```

Directory structure within `assets/` is preserved.

## Changing the Assets Directory

By default, Simple Gal looks for a directory called `assets`. You can change this in `config.toml`:

```toml
assets_dir = "site-assets"
```

The path is relative to the content root.

## Favicons

Simple Gal ships with a default `favicon.png`. To use your own, place a favicon file in `assets/`. The build detects favicon files in this priority order:

| Priority | File | MIME type |
|----------|------|-----------|
| 1 | `favicon.svg` | `image/svg+xml` |
| 2 | `favicon.ico` | `image/x-icon` |
| 3 | `favicon.png` | `image/png` |

The first match is injected as a `<link rel="icon">` tag in every page. Since assets are copied to the output root *after* the default `favicon.png` is written, placing your own `favicon.png` in `assets/` replaces the default.

For best results, use an SVG favicon. It scales to any size and supports dark mode via CSS `prefers-color-scheme` media queries inside the SVG.

## PWA Icons

Simple Gal generates a Progressive Web App manifest with default icons. To use your own, place these files in `assets/`:

| File | Size | Purpose |
|------|------|---------|
| `icon-192.png` | 192x192 px | Android home screen icon |
| `icon-512.png` | 512x512 px | Android splash screen |
| `apple-touch-icon.png` | 180x180 px | iOS home screen icon |

As with favicons, your files in `assets/` overwrite the defaults because assets are copied after the built-in icons are written.

## Custom Fonts

To use a locally hosted font instead of a Google Font:

1. Place font files in `assets/fonts/`:

```
assets/
  fonts/
    garamond.woff2
    garamond-italic.woff2
```

2. Declare the font face in `assets/custom.css`:

```css
@font-face {
    font-family: 'EB Garamond';
    src: url('/fonts/garamond.woff2') format('woff2');
    font-weight: 400;
    font-style: normal;
    font-display: swap;
}

@font-face {
    font-family: 'EB Garamond';
    src: url('/fonts/garamond-italic.woff2') format('woff2');
    font-weight: 400;
    font-style: italic;
    font-display: swap;
}
```

3. Set the font family in `config.toml`:

```toml
[font]
family = "EB Garamond"
source = "local"
```

Using `source = "local"` tells Simple Gal not to generate a Google Fonts `<link>` tag. The `@font-face` rules in `custom.css` handle loading instead.

## Other Static Files

Any file you place in `assets/` is served at the output root. Common uses:

- **`robots.txt`** -- search engine directives
- **`_headers`** or **`_redirects`** -- Netlify/Cloudflare Pages configuration
- **`CNAME`** -- GitHub Pages custom domain
- **`og-image.jpg`** -- a shared Open Graph image referenced from `head.html`

## How Copying Works

The build pipeline writes default files (favicon, PWA icons, service worker) to the output directory first, then copies the contents of `assets/` on top. This means:

1. Any file in `assets/` with the same name as a default file replaces it.
2. Files in subdirectories of `assets/` are placed in matching subdirectories in the output.
3. The only exception: `manifest.json` files are skipped during the copy to avoid conflicts with the generated `site.webmanifest`.
