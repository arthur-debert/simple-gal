# Fonts

Simple Gal supports two ways to load fonts: from Google Fonts (the default) or from a local font file. Both methods produce the same CSS custom properties and fallback stack.

## Google Fonts (default)

By default, Simple Gal loads the font from Google Fonts via a `<link>` stylesheet tag. Configure the family name and weight:

```toml
[font]
font = "Space Grotesk"
weight = "600"
font_type = "sans"
```

This generates a `<link>` tag pointing to:

```
https://fonts.googleapis.com/css2?family=Space+Grotesk:wght@600&display=swap
```

The `display=swap` parameter ensures text remains visible while the font loads.

### Choosing a Google Font

Any family available on [fonts.google.com](https://fonts.google.com) works. Set `font` to the exact family name as it appears on Google Fonts. Examples:

```toml
# Clean geometric sans-serif
[font]
font = "Space Grotesk"
weight = "600"
font_type = "sans"

# Elegant serif
[font]
font = "Playfair Display"
weight = "400"
font_type = "serif"

# Minimal sans-serif
[font]
font = "Inter"
weight = "500"
font_type = "sans"

# Monospace
[font]
font = "JetBrains Mono"
weight = "400"
font_type = "sans"
```

## Local fonts

To use a self-hosted font file instead of Google Fonts, add the `source` key pointing to the font file path relative to the site root:

```toml
[font]
font = "My Custom Font"
weight = "400"
font_type = "sans"
source = "fonts/MyFont.woff2"
```

When `source` is set:
- No Google Fonts `<link>` tag is generated.
- A `@font-face` CSS declaration is generated inline.
- The font file must be placed in your assets directory so it gets copied to the output.

### Setting up a local font

1. Create a `fonts/` directory inside your assets directory:

    ```text
    content/
    └── assets/
        └── fonts/
            └── MyFont.woff2
    ```

2. Configure the font in your `config.toml`:

    ```toml
    [font]
    font = "My Custom Font"
    weight = "400"
    font_type = "serif"
    source = "fonts/MyFont.woff2"
    ```

3. The generated CSS will include:

    ```css
    @font-face {
        font-family: "My Custom Font";
        src: url("/fonts/MyFont.woff2") format("woff2");
        font-weight: 400;
        font-display: swap;
    }

    :root {
        --font-family: "My Custom Font", Georgia, "Times New Roman", serif;
        --font-weight: 400;
    }
    ```

### Supported font formats

| Extension | CSS format | Notes |
|---|---|---|
| `.woff2` | `woff2` | Recommended. Best compression, wide browser support. |
| `.woff` | `woff` | Good compression, universal support. |
| `.ttf` | `truetype` | Larger files, universal support. |
| `.otf` | `opentype` | Similar to TTF, supports advanced typographic features. |

Use `.woff2` when possible for the smallest file size.

## Font type and fallback stacks

The `font_type` setting determines which system fonts are used as fallbacks while the custom font loads or if it fails to load:

| `font_type` | Fallback stack |
|---|---|
| `"sans"` | `Helvetica, Verdana, sans-serif` |
| `"serif"` | `Georgia, "Times New Roman", serif` |

The full `--font-family` CSS variable includes both the configured font name and the fallback stack:

```css
/* font_type = "sans" */
--font-family: "Space Grotesk", Helvetica, Verdana, sans-serif;

/* font_type = "serif" */
--font-family: "Playfair Display", Georgia, "Times New Roman", serif;
```

Choose the `font_type` that best matches the character of your chosen font. This ensures the fallback text has a similar feel if the primary font is unavailable.

## Font weight

The `weight` key is a string (not a number) that specifies which weight to load. Common values:

| Weight | Name |
|---|---|
| `"100"` | Thin |
| `"200"` | Extra Light |
| `"300"` | Light |
| `"400"` | Regular |
| `"500"` | Medium |
| `"600"` | Semi Bold |
| `"700"` | Bold |
| `"800"` | Extra Bold |
| `"900"` | Black |

The weight applies site-wide. Simple Gal loads a single weight to keep page loads fast.

## Per-album font overrides

Because configuration cascades, you can set a different font for specific albums or groups:

```toml
# content/010-Portraits/config.toml
[font]
font = "Cormorant Garamond"
weight = "300"
font_type = "serif"
```

This album uses a different font while inheriting all other settings from the parent config. Albums without a `[font]` section inherit the font from their parent.
