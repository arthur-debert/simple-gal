# System Dependencies

LightTable requires these system-level tools for image processing.

## Required

| Tool | Version | Purpose |
|------|---------|---------|
| ImageMagick | 7.x | Resizing, format conversion, EXIF handling |
| libavif | 1.x | AVIF encoding (via ImageMagick) |
| libwebp | 1.x | WebP encoding (via ImageMagick) |

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
# Check ImageMagick with AVIF/WebP support
magick -list format | grep -E 'AVIF|WEBP'
```

Expected output should show both AVIF and WEBP formats.

## GitHub Actions

The workflow uses `scripts/install-deps.sh` which handles Ubuntu installation.
