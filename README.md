# Simple Gal

A minimal static site generator for fine art photography portfolios. Generates fast-loading, responsive photo albums from a filesystem-based data source.

## How It Works

Simple Gal uses your filesystem as the data source. Directories become albums, images are ordered by numeric prefix, and markdown files become pages.

```
content/                         # Root directory
├── config.toml                  # Site configuration (optional)
├── 040-about.md                 # Page (numbered = shown in nav)
├── 050-github.md                # External link (URL-only .md file)
├── 010-Landscapes/              # Album (numbered = shown in nav)
│   ├── info.txt                 # Optional description
│   ├── 001-dawn.jpg             # Preview image (lowest number)
│   ├── 002-sunset.jpg
│   └── 010-mountains.jpg        # Non-contiguous numbering OK
├── 020-Travel/                  # Container directory (has subdirs)
│   ├── 010-Japan/               # Nested album
│   │   ├── info.txt
│   │   └── 001-tokyo.jpg
│   └── 020-Italy/
│       └── 001-rome.jpg
├── 030-Minimal/                 # Another album
│   └── 001-solo.jpg
└── wip-experiments/             # No number prefix = not in nav (still accessible)
    └── 001-draft.jpg
```

### Naming Convention

All entities (albums, images, pages, container directories) follow the same `NNN-name` convention:

- `NNN-` numeric prefix determines sort order (e.g., `001-`, `020-`, `100-`)
- Dashes in the name portion become spaces in display titles (`010-My-Best-Photos` → "My Best Photos")
- Only numbered entries appear in the navigation menu
- Unnumbered entries still exist and are accessible by URL, but are hidden from nav

### Albums

- A directory containing images (`.jpg`, `.jpeg`, `.png`, `.webp`)
- Optional `info.txt` for a description shown on the album page
- **Preview image**: Image numbered `001-*` is used as the album thumbnail. Falls back to the first image by sort order if no `001` exists.
- Directories cannot mix images and subdirectories

### Container Directories

- A directory containing other directories (not images)
- Numbered containers appear in nav as groups with their children nested underneath
- Unnumbered containers are transparent: their children are promoted to the parent level in nav

### Pages

Markdown files (`.md`) in the content root become site pages:

- **Content pages**: Regular markdown rendered as HTML pages (e.g., `040-about.md`)
- **Link pages**: If the `.md` file contains only a URL, it renders as an external link in the nav (e.g., `050-github.md` containing `https://github.com/user/repo`)
- Pages appear in the navigation **after** albums, separated by a divider
- An `# H1` heading overrides the filename-derived title

### Navigation Order

Navigation items are sorted by their numeric prefix. Albums and containers appear first, then a separator, then pages. Within each group, items are sorted by number.

## Build Pipeline

```
1. Scan      →  manifest.json    (filesystem → structured data)
2. Process   →  processed/       (responsive sizes + thumbnails)
3. Generate  →  dist/            (final HTML site)
```

Each stage is independent and produces a manifest file that the next stage consumes. Image processing is cached — unchanged sources are skipped on subsequent builds.

### Image Processing

Optimized for fine art photography:

- **Formats**: AVIF (primary) + WebP (fallback)
- **Responsive sizes**: 800px, 1400px, 2080px (configurable)
- **Thumbnails**: Single size, cropped to configured aspect ratio (default 4:5)
- **Quality**: 90% compression, EXIF preserved

### Frontend

Pure HTML/CSS with minimal JS (89 lines):

- No frameworks, no build tools, no npm
- CSS custom properties for theming
- Dark/light/auto color schemes (respects `prefers-color-scheme`)
- Stories-style navigation (click/swipe left/right edges)
- Keyboard navigation (arrow keys)
- Responsive frames that preserve aspect ratio
- View transitions (where supported)

## Installation

### Dependencies

- **Rust compiler** (for building the CLI)
- **ImageMagick** (`convert` and `identify` commands) with AVIF and WebP support

See [DEPENDENCIES.md](DEPENDENCIES.md) for platform-specific installation instructions, or run:

```bash
./scripts/install-deps.sh
```

### Build

```bash
cargo build --release
```

## Usage

```bash
# Full build (defaults: --source content --output dist)
simple-gal build

# Override paths
simple-gal --source photos --output public build

# Run stages individually
simple-gal scan
simple-gal process
simple-gal generate
```

### CLI Options

```
Options:
  --source <DIR>      Content directory           [default: content]
  --output <DIR>      Output directory             [default: dist]
  --temp-dir <DIR>    Intermediate files directory  [default: .simple-gal-temp]
```

All options are global and shared across subcommands. Intermediate files (manifests, processed images) are stored in `--temp-dir` and preserved between builds for caching and debugging.

### Build Script

```bash
# Full build using the same script as CI
./scripts/build.sh
```

## Configuration

Place `config.toml` in your content root (e.g., `content/config.toml`). All options are optional — defaults are used for any missing values.

```toml
# Path to content directory (resolved relative to CWD)
content_root = "content"

[thumbnails]
aspect_ratio = [4, 5]     # width:height

[images]
sizes = [800, 1400, 2080] # Responsive sizes to generate
quality = 90              # AVIF/WebP quality (0-100)

[theme.frame_x]
size = "3vw"              # Preferred horizontal frame size
min = "1rem"              # Minimum horizontal frame size
max = "2.5rem"            # Maximum horizontal frame size

[theme.frame_y]
size = "6vw"              # Preferred vertical frame size
min = "2rem"              # Minimum vertical frame size
max = "5rem"              # Maximum vertical frame size

[colors.light]
background = "#ffffff"
text = "#111111"
text_muted = "#666666"    # Nav, breadcrumbs, captions
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

[processing]
max_processes = 4             # Max parallel workers (omit for auto = CPU cores)
```

Partial configuration is supported — override only the values you want to change:

```toml
[colors.light]
background = "#fafafa"
```

## UI Behavior

### Album View
- Title + optional description
- Grid of thumbnails (no pagination, controlled album sizes)

### Image View
- Breadcrumb navigation: Home > Album
- Image in responsive frame with configurable padding
- Navigation: click/tap right edge → next, left edge → previous
- Keyboard: left/right arrow keys
- First image ← goes to album, last image → goes to album

### Color Schemes
- Respects `prefers-color-scheme`
- Light: white background, dark text
- Dark: near-black background, light text

## Development

```bash
# Run tests
cargo test

# Build and preview locally
./scripts/build.sh
python -m http.server -d dist
```

## Project Structure

```
├── src/
│   ├── main.rs           # CLI entry point
│   ├── naming.rs         # Filename parsing (NNN-name convention)
│   ├── types.rs          # Shared types (Page, NavItem)
│   ├── config.rs         # Site configuration (config.toml)
│   ├── scan.rs           # Stage 1: filesystem → manifest
│   ├── process.rs        # Stage 2: image processing
│   ├── generate.rs       # Stage 3: HTML generation
│   └── imaging/          # ImageMagick backend
├── static/
│   ├── style.css         # Base styles (inlined at build)
│   └── nav.js            # Keyboard/touch navigation (inlined at build)
├── scripts/
│   ├── build.sh          # Full build (used by CI)
│   └── install-deps.sh   # System dependency installer
└── fixtures/             # Test data
```

## Deployment

GitHub Actions workflow:
1. Triggers on push to main
2. Builds the CLI and runs the full pipeline
3. Publishes to GitHub Pages
