# Configuration Reference

Every configuration key, its type, default value, and purpose. All keys are optional. Run `simple-gal gen-config` to generate a complete annotated config file.

## Top-level keys

| Key | Type | Default | Description |
|---|---|---|---|
| `content_root` | string | `"content"` | Path to the content directory. Only meaningful in the root config. |
| `site_title` | string | `"Gallery"` | Site title used in breadcrumbs and the browser tab for the index page. |
| `assets_dir` | string | `"assets"` | Directory for static assets (favicon, fonts, etc.), relative to content root. Contents are copied verbatim to the output root. Silently skipped if it does not exist. |
| `site_description_file` | string | `"site"` | Stem of the site description file in the content root. If `site.md` or `site.txt` exists, its content is rendered on the index page. |

```toml
content_root = "content"
site_title = "My Portfolio"
assets_dir = "assets"
site_description_file = "site"
```

## `[thumbnails]`

Controls how thumbnails are cropped and sized.

| Key | Type | Default | Description |
|---|---|---|---|
| `aspect_ratio` | `[u32, u32]` | `[4, 5]` | Width-to-height ratio for thumbnail crops. `[1, 1]` for square, `[3, 2]` for landscape. |
| `size` | `u32` | `400` | Short-edge size in pixels for generated thumbnails. |

```toml
[thumbnails]
aspect_ratio = [4, 5]
size = 400
```

Common aspect ratio choices:

| Ratio | Effect |
|---|---|
| `[1, 1]` | Square thumbnails |
| `[4, 5]` | Slightly tall portrait (default) |
| `[3, 2]` | Classic landscape |
| `[2, 3]` | Tall portrait |

## `[images]`

Controls responsive image generation.

| Key | Type | Default | Description |
|---|---|---|---|
| `sizes` | `[u32, ...]` | `[800, 1400, 2080]` | Pixel widths (longer edge) to generate for responsive `<picture>` elements. |
| `quality` | `u32` | `90` | AVIF encoding quality. 0 = smallest file / worst quality, 100 = largest file / best quality. |

```toml
[images]
sizes = [800, 1400, 2080]
quality = 90
```

Validation rules:
- `quality` must be 0--100.
- `sizes` must contain at least one value.

## `[theme]`

Layout spacing values. All values are CSS length strings.

| Key | Type | Default | Description |
|---|---|---|---|
| `thumbnail_gap` | string | `"1rem"` | Gap between thumbnails in album and image grids. |
| `grid_padding` | string | `"2rem"` | Padding around the thumbnail grid container. |

```toml
[theme]
thumbnail_gap = "1rem"
grid_padding = "2rem"
```

### `[theme.mat_x]`

Horizontal mat (spacing) around images on photo pages. Rendered as CSS `clamp(min, size, max)`.

| Key | Type | Default | Description |
|---|---|---|---|
| `size` | string | `"3vw"` | Preferred/fluid value, typically viewport-relative. |
| `min` | string | `"1rem"` | Minimum bound. |
| `max` | string | `"2.5rem"` | Maximum bound. |

```toml
[theme.mat_x]
size = "3vw"
min = "1rem"
max = "2.5rem"
```

### `[theme.mat_y]`

Vertical mat (spacing) around images on photo pages. Same structure as `mat_x`.

| Key | Type | Default | Description |
|---|---|---|---|
| `size` | string | `"6vw"` | Preferred/fluid value. |
| `min` | string | `"2rem"` | Minimum bound. |
| `max` | string | `"5rem"` | Maximum bound. |

```toml
[theme.mat_y]
size = "6vw"
min = "2rem"
max = "5rem"
```

## `[colors.light]`

Light mode color scheme. Applied by default and when the user's system is set to light mode.

| Key | Type | Default | Description |
|---|---|---|---|
| `background` | string | `"#ffffff"` | Page background color. |
| `text` | string | `"#111111"` | Primary text color. |
| `text_muted` | string | `"#666666"` | Secondary text: nav menu, breadcrumbs, captions. |
| `border` | string | `"#e0e0e0"` | Border color. |
| `separator` | string | `"#e0e0e0"` | Separator color: header underline, nav menu divider. |
| `link` | string | `"#333333"` | Link color. |
| `link_hover` | string | `"#000000"` | Link hover color. |

```toml
[colors.light]
background = "#ffffff"
text = "#111111"
text_muted = "#666666"
border = "#e0e0e0"
separator = "#e0e0e0"
link = "#333333"
link_hover = "#000000"
```

## `[colors.dark]`

Dark mode color scheme. Applied when the user's system prefers dark mode (`prefers-color-scheme: dark`).

| Key | Type | Default | Description |
|---|---|---|---|
| `background` | string | `"#000000"` | Page background color. |
| `text` | string | `"#fafafa"` | Primary text color. |
| `text_muted` | string | `"#999999"` | Secondary text. |
| `border` | string | `"#333333"` | Border color. |
| `separator` | string | `"#333333"` | Separator color. |
| `link` | string | `"#cccccc"` | Link color. |
| `link_hover` | string | `"#ffffff"` | Link hover color. |

```toml
[colors.dark]
background = "#000000"
text = "#fafafa"
text_muted = "#999999"
border = "#333333"
separator = "#333333"
link = "#cccccc"
link_hover = "#ffffff"
```

## `[font]`

Typography settings. By default, fonts are loaded from Google Fonts. Set `source` to use a local font file instead.

| Key | Type | Default | Description |
|---|---|---|---|
| `font` | string | `"Space Grotesk"` | Font family name. Used as the Google Fonts family name, or as the `font-family` name for local fonts. |
| `weight` | string | `"600"` | Font weight to load. |
| `font_type` | string | `"sans"` | `"sans"` or `"serif"`. Determines the CSS fallback font stack. |
| `source` | string | *(none)* | Path to a local font file, relative to the site root. When set, generates `@font-face` CSS instead of loading from Google Fonts. Supported formats: `.woff2`, `.woff`, `.ttf`, `.otf`. |

```toml
# Google Fonts (default behavior)
[font]
font = "Space Grotesk"
weight = "600"
font_type = "sans"

# Local font file
[font]
font = "My Custom Font"
weight = "400"
font_type = "sans"
source = "fonts/MyFont.woff2"
```

## `[processing]`

Parallel image processing settings.

| Key | Type | Default | Description |
|---|---|---|---|
| `max_processes` | u32 | *(auto: CPU core count)* | Maximum number of parallel image processing workers. When omitted, uses all available CPU cores. Values larger than the core count are clamped down. |

```toml
[processing]
max_processes = 4
```

## CSS custom properties

Config values are compiled into CSS custom properties, injected as inline `<style>` blocks in every page. The stylesheet references these variables rather than hardcoded values.

### Color variables

| CSS variable | Config key |
|---|---|
| `--color-bg` | `colors.{light,dark}.background` |
| `--color-text` | `colors.{light,dark}.text` |
| `--color-text-muted` | `colors.{light,dark}.text_muted` |
| `--color-border` | `colors.{light,dark}.border` |
| `--color-separator` | `colors.{light,dark}.separator` |
| `--color-link` | `colors.{light,dark}.link` |
| `--color-link-hover` | `colors.{light,dark}.link_hover` |

Light mode values are set on `:root`. Dark mode values are set inside `@media (prefers-color-scheme: dark)`.

### Theme variables

| CSS variable | Config key | Generated as |
|---|---|---|
| `--mat-x` | `theme.mat_x.*` | `clamp(min, size, max)` |
| `--mat-y` | `theme.mat_y.*` | `clamp(min, size, max)` |
| `--thumbnail-gap` | `theme.thumbnail_gap` | Direct value |
| `--grid-padding` | `theme.grid_padding` | Direct value |

### Font variables

| CSS variable | Config key |
|---|---|
| `--font-family` | `font.font` + `font.font_type` (includes fallback stack) |
| `--font-weight` | `font.weight` |
