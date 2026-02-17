# Local Usage

## Installation

**From crates.io:**

```bash
cargo install simple-gal
```

**From GitHub releases:**

Download a pre-built binary for your platform from the [releases page](https://github.com/arthur-debert/simple-gal/releases). Place it somewhere on your `PATH`.

## Building your site

The default command reads from `content/` and writes to `dist/`:

```bash
simple-gal build
```

Override the input and output directories with flags:

```bash
simple-gal build --source photos --output public
```

This processes all images and generates the complete static site in the output directory.

## CLI commands

| Command | What it does |
|---------|-------------|
| `simple-gal build` | Run the full pipeline: scan, process images, generate HTML |
| `simple-gal scan` | Scan the content directory and print the manifest (no image processing or HTML output) |
| `simple-gal process` | Scan and process images (generate responsive sizes and thumbnails) without generating HTML |
| `simple-gal generate` | Scan, process, and generate HTML (same as `build`) |
| `simple-gal gen-config` | Print a fully-commented `config.toml` with all stock defaults |

The individual stage commands (`scan`, `process`) are useful for debugging. In normal use, `build` is all you need.

## Generating a starter config

To see every available configuration option with its default value:

```bash
simple-gal gen-config > content/config.toml
```

Edit the generated file to keep only the settings you want to customize. See [Configuration Overview](../configuration/overview.md) for details on how config merging works.

## Previewing locally

The output in `dist/` is a static site. Any local HTTP server will work for previewing. A few options:

```bash
# Python (built into macOS and most Linux)
python3 -m http.server --directory dist 8000

# Node.js
npx serve dist

# PHP
php -S localhost:8000 -t dist
```

Then open `http://localhost:8000` in your browser.

> **Note:** PWA features (service worker, offline mode) require HTTPS in production, but work over `localhost` during development.
