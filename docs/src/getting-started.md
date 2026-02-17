# Quick Start

This guide takes you from zero to a working photography gallery in about five minutes.

## Install

Grab a pre-built binary from the [releases page](https://github.com/arthur-debert/simple-gal/releases) for your platform, or install via Cargo:

```bash
cargo install simple-gal
```

Verify the installation:

```bash
simple-gal --version
```

## Create your content

Simple Gal reads from a `content/` directory. Create one with an album inside it:

```bash
mkdir -p content/010-My-Album
```

Copy some JPEG or PNG files into that album directory:

```bash
cp ~/Photos/favorites/*.jpg content/010-My-Album/
```

That's it. The directory name becomes the album title (`My-Album`), and the numeric prefix (`010`) controls where it appears in navigation.

## Build

Run the build command from the directory that contains `content/`:

```bash
simple-gal build
```

You'll see output like this:

```text
Scanning content...
Found 1 album, 12 images
Processing images...
Generating site...
Done. Output: dist/
```

The `dist/` directory now contains a complete, self-contained gallery site.

## View

Open the generated site in your browser:

```bash
open dist/index.html
```

You should see a home page with a thumbnail grid for your album. Click into the album to browse photos with keyboard arrows, swipe gestures, or edge clicks.

## Add a site title

By default the site has no title. Create a configuration file to set one:

```bash
echo 'site_title = "My Portfolio"' > content/config.toml
```

Rebuild:

```bash
simple-gal build
```

The title now appears in the header and in the browser tab.

## Add more albums

Create additional directories with numeric prefixes to control their order:

```bash
mkdir content/020-Portraits
mkdir content/030-Travel
```

Drop images into each one and rebuild. The home page will show one thumbnail per album, ordered by prefix.

## Add an album description

Create a `description.md` (or `description.txt`) file inside any album directory:

```bash
echo "Landscapes from the Pacific Northwest, 2019--2024." > content/010-My-Album/description.txt
```

The text appears above the thumbnail grid on that album's page.

## Add a page

Markdown files in the content root with a numeric prefix become pages in the site navigation:

```bash
cat > content/040-About.md << 'EOF'
# About

Photographer based in Portland, Oregon.
Contact: photos@example.com
EOF
```

Rebuild, and "About" appears in the navigation bar.

## Explore the full config

Run `gen-config` to see every available option with its default value:

```bash
simple-gal gen-config
```

Redirect it to a file to use as a starting point:

```bash
simple-gal gen-config > content/config.toml
```

From here, you can customize [colors and theme](configuration/colors-and-theme.md), [fonts](configuration/fonts.md), [thumbnail aspect ratios](images/thumbnails.md), [image quality](images/quality.md), and more. Each album can override any setting with its own `config.toml` -- only the keys you change need to be listed.

## Next steps

- [Directory Structure](content/directory-structure.md) -- how the filesystem maps to your site
- [Configuration Overview](configuration/overview.md) -- how config cascading works
- [Deployment](deployment/local.md) -- putting your gallery online
- [The Forever Stack](philosophy/forever-stack.md) -- why it's built this way
