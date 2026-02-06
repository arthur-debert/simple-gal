#!/usr/bin/env bash
set -euo pipefail

# Install system dependencies for Simple Gal
# Detects OS and installs appropriately
#
# Required commands after install:
# - convert (ImageMagick)
# - identify (ImageMagick)

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

    # Check for convert command
    if ! command -v convert &> /dev/null; then
        echo "Error: 'convert' command not found"
        echo "ImageMagick must be installed with the 'convert' and 'identify' commands"
        exit 1
    fi
    echo "Found: convert"

    # Check for identify command
    if ! command -v identify &> /dev/null; then
        echo "Error: 'identify' command not found"
        echo "ImageMagick must be installed with the 'convert' and 'identify' commands"
        exit 1
    fi
    echo "Found: identify"

    # Check for AVIF support
    if ! convert -list format | grep -q AVIF; then
        echo "Warning: ImageMagick lacks AVIF support"
        exit 1
    fi
    echo "Found: AVIF support"

    # Check for WebP support
    if ! convert -list format | grep -q WEBP; then
        echo "Error: ImageMagick lacks WebP support"
        exit 1
    fi
    echo "Found: WebP support"

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
