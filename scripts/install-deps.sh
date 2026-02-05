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

    if ! command -v magick &> /dev/null; then
        echo "Error: ImageMagick (magick) not found"
        exit 1
    fi

    # Check for AVIF support
    if ! magick -list format | grep -q AVIF; then
        echo "Error: ImageMagick lacks AVIF support"
        exit 1
    fi

    # Check for WebP support
    if ! magick -list format | grep -q WEBP; then
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
