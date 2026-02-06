# LightTable

A minimal static site generator for fine art photography portfolios. Generates fast-loading, responsive photo albums from a filesystem-based data source.

## How It Works

LightTable uses your filesystem as the data source:

```
images/                          # Root directory
├── 010-Landscapes/              # Album (numbered = shown in nav)
│   ├── info.txt                 # Optional description
│   ├── 001-sunset.jpg           # Images ordered by number prefix
│   ├── 002-mountains.jpg
│   └── 010-forest.jpg           # Non-contiguous numbering OK
├── 020-Portraits/               # Another album
│   └── ...
├── 030-Travel/                  # Directory containing albums
│   ├── 010-Japan/               # Nested album
│   │   └── ...
│   └── 020-Italy/
│       └── ...
└── wip-experiments/             # No number prefix = not in nav (still accessible)
    └── ...
```

### Rules

- **Albums**: Directories containing images (`.jpg`, `.jpeg`, `.png`, `.webp`)
- **Ordering**: `NNN-` prefix determines sort order (e.g., `001-`, `020-`, `100-`)
- **Navigation**: Only numbered directories appear in the nav menu
- **Nesting**: Directories can contain albums OR other directories, not both
- **Preview**: Image `001-*` is used as the album thumbnail
- **Description**: Optional `info.txt` in album directory

## Technical Stack

### Build Pipeline

```
1. Scan        →  manifest.json    (filesystem → structured data)
2. Process     →  images/          (responsive sizes + thumbnails)
3. Generate    →  dist/            (final HTML site)
```

Each stage is independent. Image processing is cached—unchanged sources are skipped.

### Image Handling

Optimized for fine art photography:

- **Formats**: AVIF (primary) + WebP (fallback)
- **Sizes**: 800px, 1400px, 2080px (3 responsive variants)
- **Thumbnails**: Single size, cropped to configured aspect ratio (default 4:5)
- **Quality**: 90% compression, minimal sharpening, EXIF preserved

### Frontend

Pure HTML/CSS with minimal JS (~80 lines):

- No frameworks, no build tools, no npm
- CSS custom properties for theming
- Dark/light/auto color schemes
- Stories-style navigation (click/swipe left/right edges)
- Responsive frames that preserve aspect ratio

### Deployment

GitHub Actions workflow:
1. Triggers on push to main
2. Runs the same build scripts used locally
3. Publishes to GitHub Pages

## Installation

```bash
# Build the CLI
cargo build --release

# Run from images root
lighttable build ./images --output ./dist

# Or use the build script (same as CI)
./scripts/build.sh
```

## Configuration

`config.toml` in your content root (e.g. `images/config.toml`). All options are optional — defaults are used for any missing values.

```toml
[thumbnails]
aspect_ratio = [4, 5]  # width:height

[images]
max_size = 2080
sizes = [800, 1400, 2080]
quality = 90

[theme]
frame_width = "clamp(1rem, 3vw, 2.5rem)"

[colors.light]
background = "#ffffff"
text = "#111111"
text_muted = "#666666"
border = "#e0e0e0"
link = "#333333"
link_hover = "#000000"

[colors.dark]
background = "#0a0a0a"
text = "#eeeeee"
text_muted = "#999999"
border = "#333333"
link = "#cccccc"
link_hover = "#ffffff"
```

## UI Behavior

### Album View
- Title + optional description
- Grid of thumbnails (no pagination, controlled album sizes)

### Image View
- Navigation breadcrumb: Home > Album
- Image in responsive frame
- Navigation: click/tap right edge → next, left edge → previous
- Keyboard: ← → arrows
- First image ← goes to album, last image → goes to album

### Color Schemes
- Respects `prefers-color-scheme`
- Light: white background
- Dark: near-black background

## Development

```bash
# Run tests (uses fixtures copied to /tmp)
cargo test

# Build locally
./scripts/build.sh

# Preview
python -m http.server -d dist
```

## Project Structure

```
├── src/
│   ├── main.rs           # CLI entry point
│   ├── scan.rs           # Stage 1: filesystem → manifest
│   ├── process.rs        # Stage 2: image processing
│   └── generate.rs       # Stage 3: HTML generation
├── scripts/
│   ├── build.sh          # Full build (used by CI)
│   └── process-images.sh # Image processing wrapper
├── fixtures/             # Test data
├── templates/            # HTML templates
└── static/               # CSS, JS (inlined at build)
```
