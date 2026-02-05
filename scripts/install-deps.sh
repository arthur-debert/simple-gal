#!/usr/bin/env bash
set -euo pipefail

# Install system dependencies for LightTable
# Detects OS and installs appropriately

install_macos() {
    echo "==> Installing dependencies via Homebrew"
    if ! command -v brew &> /dev/null; then
        echo "Error: Homebrew not found. Install from https://brew.sh"
        exit 1
    fi
    brew install imagemagick webp libavif
}

install_ubuntu() {
    echo "==> Installing dependencies via apt"
    sudo apt-get update
    sudo apt-get install -y imagemagick webp libavif-bin
}

install_arch() {
    echo "==> Installing dependencies via pacman"
    sudo pacman -S --noconfirm imagemagick libwebp libavif
}

verify_install() {
    echo "==> Verifying installation"

    # Check for ImageMagick (either magick or convert)
    if command -v magick &> /dev/null; then
        IM_CMD="magick"
    elif command -v convert &> /dev/null; then
        IM_CMD="convert"
    else
        echo "Error: ImageMagick not found"
        exit 1
    fi
    echo "Found ImageMagick: $IM_CMD"

    # Check for AVIF support
    if ! $IM_CMD -list format | grep -q AVIF; then
        echo "Warning: ImageMagick lacks AVIF support, will use WebP only"
    fi

    # Check for WebP support
    if ! $IM_CMD -list format | grep -q WEBP; then
        echo "Error: ImageMagick lacks WebP support"
        exit 1
    fi

    echo "==> All dependencies installed and verified"
}

# Detect OS
case "$(uname -s)" in
    Darwin)
        install_macos
        ;;
    Linux)
        if [ -f /etc/arch-release ]; then
            install_arch
        elif [ -f /etc/debian_version ]; then
            install_ubuntu
        else
            echo "Error: Unsupported Linux distribution"
            echo "Please install manually: imagemagick, webp, libavif"
            exit 1
        fi
        ;;
    *)
        echo "Error: Unsupported OS"
        exit 1
        ;;
esac

verify_install
