# Configuration Overview

Simple Gal uses TOML configuration files that cascade through the directory tree. Each level inherits settings from its parent and can override specific keys without repeating the rest.

## The config chain

Configuration is resolved in four layers, from least to most specific:

1. **Stock defaults** -- built into the binary, always present
2. **Root `config.toml`** -- in your content directory root
3. **Group `config.toml`** -- in a group (nested album) directory
4. **Gallery `config.toml`** -- in an individual album directory

Each layer merges on top of the previous one. Only the keys you specify are overridden; everything else passes through unchanged.

```text
Stock defaults
  └─ content/config.toml              (root overrides)
       └─ content/020-Travel/config.toml       (group overrides)
            └─ content/020-Travel/010-Japan/config.toml  (gallery overrides)
```

## Partial configs

Config files are sparse. You never need to specify every key -- just the ones you want to change. For example, a gallery that only needs different AVIF quality:

```toml
# content/020-Travel/010-Japan/config.toml
[images]
quality = 75
```

This gallery inherits all other settings (colors, fonts, thumbnail ratios, theme spacing) from its parent group, which in turn inherits from the root, which inherits from stock defaults.

## Merge example

Consider this directory tree:

```text
content/
├── config.toml
├── 010-Portraits/
│   ├── photo1.jpg
│   └── photo2.jpg
└── 020-Travel/
    ├── config.toml
    └── 010-Japan/
        ├── config.toml
        └── photo1.jpg
```

**Root config** (`content/config.toml`):

```toml
site_title = "My Portfolio"

[font]
font = "Playfair Display"
font_type = "serif"
weight = "400"

[thumbnails]
aspect_ratio = [3, 4]
```

**Group config** (`content/020-Travel/config.toml`):

```toml
[thumbnails]
aspect_ratio = [1, 1]
```

**Gallery config** (`content/020-Travel/010-Japan/config.toml`):

```toml
[images]
quality = 75
```

Here is what each level sees:

| Setting | 010-Portraits | 020-Travel | 020-Travel/010-Japan |
|---|---|---|---|
| `site_title` | `"My Portfolio"` | `"My Portfolio"` | `"My Portfolio"` |
| `font.font` | `"Playfair Display"` | `"Playfair Display"` | `"Playfair Display"` |
| `thumbnails.aspect_ratio` | `[3, 4]` | `[1, 1]` | `[1, 1]` |
| `images.quality` | `90` (stock default) | `90` (stock default) | `75` |

The **010-Portraits** album has no `config.toml`, so it uses the root config as-is. The **020-Travel** group overrides only the thumbnail aspect ratio; everything else flows through from root. The **010-Japan** gallery inherits the square thumbnails from its parent group and overrides only the image quality.

## Merge rules

- **Scalar values** (strings, numbers): the child value replaces the parent value.
- **Arrays** (like `images.sizes`): the child array replaces the parent array entirely. There is no element-level merging.
- **Subsections** (like `[colors.light]`): merged key by key. Setting `background` in a child does not reset `text` or other sibling keys.
- **Absent keys**: inherited from the parent level without change.

## Unknown key rejection

Simple Gal rejects any key it does not recognize. This catches typos before they silently produce wrong output:

```toml
[images]
qualty = 90    # Error: unknown field "qualty"
```

The error message names the unknown field and lists the valid alternatives, so the fix is usually obvious.

## Generating a starter config

Run `simple-gal gen-config` to print a fully-commented `config.toml` with every key and its stock default value:

```bash
simple-gal gen-config > content/config.toml
```

Edit the generated file to keep only the keys you want to customize, or leave them all in place as documentation. Either way works -- stock-default values are harmless to repeat.
