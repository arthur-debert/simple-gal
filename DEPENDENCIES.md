# System Dependencies

LightTable requires these system-level tools for image processing.

## Required

| Tool | Commands Used | Purpose |
|------|---------------|---------|
| ImageMagick | `convert`, `identify` | Resizing, format conversion, dimension detection |
| libavif | - | AVIF encoding (via ImageMagick) |
| libwebp | - | WebP encoding (via ImageMagick) |

## Installation

### macOS (Homebrew)

```bash
brew install imagemagick webp libavif
```

### Ubuntu / Debian

```bash
sudo apt-get update
sudo apt-get install -y imagemagick webp libavif-bin
```

### Arch Linux

```bash
sudo pacman -S imagemagick libwebp libavif
```

### Verify Installation

```bash
# Check required commands exist
convert -version
identify -version

# Check AVIF/WebP support
convert -list format | grep -E 'AVIF|WEBP'
```

Expected output should show both AVIF and WEBP formats.

## Automated Installation

Use the provided script which handles detection and verification:

```bash
./scripts/install-deps.sh
```

## GitHub Actions

The CI workflow installs dependencies via apt and verifies the `convert` command.
