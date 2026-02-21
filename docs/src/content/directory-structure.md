# Directory Structure

Simple Gal uses the filesystem as its data source. Directories become albums, images become photos, and the hierarchy you create on disk maps directly to the navigation and URL structure of the generated site.

## The content root

By default, Simple Gal looks for a `content/` directory. You can change this with the `--source` flag:

```bash
simple-gal build --source my-photos --output dist
```

Everything inside this root directory is scanned and processed.

## Full directory tree

Here is a complete example showing all content types:

```text
content/
├── config.toml                      # Site-wide configuration (optional)
├── site.md                          # Site description for the home page (optional)
├── 040-about.md                     # Page (numbered = in nav)
├── 050-github.md                    # Link page (URL-only content)
├── 010-Landscapes/                  # Album (contains images)
│   ├── config.toml                  # Per-album config override (optional)
│   ├── description.txt              # Album description (optional)
│   ├── 001-dawn.jpg                 # Image (lowest number = preview)
│   ├── 001-dawn.txt                 # Sidecar description for dawn.jpg
│   ├── 002-dusk.jpg
│   └── 010-night.jpg
├── 020-Travel/                      # Group (contains subdirectories, not images)
│   ├── config.toml                  # Group-level config (optional, cascades down)
│   ├── 010-Japan/                   # Nested album
│   │   ├── description.md           # Markdown description (takes priority over .txt)
│   │   ├── description.txt          # Plain text description (fallback)
│   │   ├── 001-tokyo.jpg
│   │   ├── 001-tokyo.txt            # Sidecar description
│   │   └── 002-kyoto.jpg
│   └── 020-Italy/
│       └── 001-rome.jpg
├── 030-Minimal/                     # Another album
│   └── 001-solo.jpg
└── wip-drafts/                      # No number prefix = hidden from nav
    └── 001-test.jpg
```

## How directories are classified

A directory becomes one of two things, determined by what it contains:

| Contains | Type | Behavior |
|----------|------|----------|
| Image files | **Album** | Generates a gallery page with thumbnails and individual photo pages |
| Subdirectories | **Group** | Generates a gallery-list page showing thumbnail cards for each child album or sub-group; appears as a clickable parent entry in navigation |

A directory cannot contain both images and subdirectories. This is enforced by the scanner and will produce an error:

```text
Error: Directory contains both images and subdirectories: content/010-Mixed
```

## Supported image formats

Simple Gal recognizes these file extensions (case-insensitive):

- `.jpg`, `.jpeg`
- `.png`
- `.tif`, `.tiff`
- `.webp`

All other files in album directories are ignored during scanning (except for special files like `description.md`, `config.toml`, and sidecar `.txt` files).

## URL structure

The generated site mirrors the directory hierarchy with number prefixes stripped:

| Filesystem path | URL | Page type |
|-----------------|-----|-----------|
| `010-Landscapes/` | `/Landscapes/` | Album (thumbnail grid) |
| `020-Travel/` | `/Travel/` | Gallery list (child album cards) |
| `020-Travel/010-Japan/` | `/Travel/Japan/` | Album (thumbnail grid) |
| `wip-drafts/` | `/wip-drafts/` | Album (thumbnail grid, hidden from nav) |

Number prefixes control ordering and navigation visibility, but they are removed from the output paths and URLs.

## Special files

These files are recognized at the content root:

| File | Purpose |
|------|---------|
| `config.toml` | Site configuration |
| `site.md` or `site.txt` | Site description rendered on the home page |
| `NNN-name.md` | Pages (appear in navigation if numbered) |

These files are recognized inside album and group directories:

| File | Purpose |
|------|---------|
| `config.toml` | Per-album/group configuration override |
| `description.md` or `description.txt` | Description shown above the thumbnail grid (albums) or gallery list (groups) |
| `NNN-name.txt` | Sidecar description for the image with the same stem (albums only) |

## Files and directories that are ignored

The scanner skips:

- Hidden files and directories (names starting with `.`)
- `processed/` and `dist/` directories (build artifacts)
- `manifest.json`
- The configured assets directory (default: `assets/`)
