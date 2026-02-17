# Colors and Theme

Simple Gal generates CSS custom properties from your color and theme configuration. Colors adapt automatically to the visitor's system preference (light or dark mode). Theme settings control the spatial layout around images and thumbnails.

## Color schemes

Two independent color schemes are defined under `[colors.light]` and `[colors.dark]`. The light scheme is the default; the dark scheme activates via the `prefers-color-scheme: dark` media query.

Each scheme has seven color slots:

| Slot | Used for |
|---|---|
| `background` | Page background |
| `text` | Primary body text |
| `text_muted` | Secondary text: navigation, breadcrumbs, captions |
| `border` | Element borders |
| `separator` | Header underline, nav menu dividers |
| `link` | Link text |
| `link_hover` | Link text on hover |

### Overriding individual colors

You do not need to redefine all seven colors. Override only the ones you want to change:

```toml
[colors.light]
background = "#f5f0eb"
text = "#2a2a2a"
```

The remaining five light-mode colors keep their stock defaults. Dark mode is entirely unaffected.

### Generated CSS

The color config produces two CSS blocks:

```css
:root {
    --color-bg: #ffffff;
    --color-text: #111111;
    --color-text-muted: #666666;
    --color-border: #e0e0e0;
    --color-link: #333333;
    --color-link-hover: #000000;
    --color-separator: #e0e0e0;
}

@media (prefers-color-scheme: dark) {
    :root {
        --color-bg: #000000;
        --color-text: #fafafa;
        --color-text-muted: #999999;
        --color-border: #333333;
        --color-link: #cccccc;
        --color-link-hover: #ffffff;
        --color-separator: #333333;
    }
}
```

These variables are referenced throughout the stylesheet. You can also use them in a `custom.css` file to style additional elements consistently.

## Mat spacing

In traditional photography, a **mat** (or mount) is the border between a print and its frame. Simple Gal uses this concept for the breathing room around full-size images on photo pages.

Two mat dimensions are configurable:

- **`mat_x`** -- horizontal spacing (left and right of the image)
- **`mat_y`** -- vertical spacing (above and below the image)

Each is defined as three values that map to CSS `clamp()`:

```toml
[theme.mat_x]
size = "3vw"      # preferred size, scales with viewport
min = "1rem"      # never smaller than this
max = "2.5rem"    # never larger than this
```

### How CSS clamp() works

`clamp(min, preferred, max)` picks the preferred value but constrains it within bounds:

- On a narrow phone screen, `3vw` might compute to `10px`, which is less than `1rem` (~16px). The mat stays at `1rem`.
- On a standard laptop, `3vw` might be `28px`, comfortably between `1rem` and `2.5rem`. The mat uses `28px`.
- On a 4K monitor, `3vw` could be `60px`, exceeding `2.5rem` (~40px). The mat caps at `2.5rem`.

This produces spacing that feels proportional on every screen without becoming too tight on phones or too wide on large displays.

The generated CSS:

```css
:root {
    --mat-x: clamp(1rem, 3vw, 2.5rem);
    --mat-y: clamp(2rem, 6vw, 5rem);
    --thumbnail-gap: 1rem;
    --grid-padding: 2rem;
}
```

### Adjusting mat size

To make the image presentation tighter (less surrounding space):

```toml
[theme.mat_x]
size = "1.5vw"
min = "0.5rem"
max = "1.5rem"

[theme.mat_y]
size = "3vw"
min = "1rem"
max = "2.5rem"
```

To make it more expansive (gallery-wall feel):

```toml
[theme.mat_x]
size = "5vw"
min = "2rem"
max = "4rem"

[theme.mat_y]
size = "8vw"
min = "3rem"
max = "6rem"
```

You can override individual fields within a mat section. For example, adjusting only the minimum without touching the preferred or maximum values:

```toml
[theme.mat_x]
min = "0.5rem"
```

## Grid spacing

Two values control the thumbnail grid layout:

| Key | Default | Effect |
|---|---|---|
| `thumbnail_gap` | `"1rem"` | Space between individual thumbnails in the grid |
| `grid_padding` | `"2rem"` | Padding around the entire grid container |

```toml
[theme]
thumbnail_gap = "0.5rem"
grid_padding = "1rem"
```

These accept any valid CSS length value: `rem`, `em`, `px`, `vw`, etc. Use `rem` for values that scale with the user's font size preference, or `px` for fixed spacing.

### Tight grid example

For a dense, mosaic-style layout with minimal spacing:

```toml
[theme]
thumbnail_gap = "2px"
grid_padding = "0"
```

### Spacious grid example

For a gallery feel with generous breathing room:

```toml
[theme]
thumbnail_gap = "1.5rem"
grid_padding = "3rem"
```

## Per-album theming

Because config files cascade through the directory tree, you can give different albums different visual treatments. A travel photography group might use tighter spacing, while a studio portrait gallery uses wider mats:

```toml
# content/020-Travel/config.toml
[theme]
thumbnail_gap = "2px"
grid_padding = "0.5rem"

[theme.mat_x]
size = "1vw"
min = "0.25rem"
max = "1rem"
```

```toml
# content/010-Portraits/config.toml
[theme.mat_x]
size = "5vw"
min = "2rem"
max = "4rem"
```

Colors, fonts, and other settings not specified in these files are inherited from the root config or stock defaults.
