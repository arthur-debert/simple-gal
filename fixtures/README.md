# Test Fixtures

This directory contains minimal test data for the filesystem scanner.

## Structure

```
content/
├── 010-Landscapes/           # Numbered album (in nav)
│   ├── info.txt
│   ├── 001-dawn.jpg
│   ├── 002-dusk.jpg
│   └── 010-night.jpg         # Non-contiguous OK
├── 020-Travel/               # Directory with nested albums
│   ├── 010-Japan/
│   │   ├── info.txt
│   │   └── 001-tokyo.jpg
│   └── 020-Italy/
│       └── 001-rome.jpg      # No info.txt (optional)
├── 030-Minimal/              # Album with single image, no info
│   └── 001-solo.jpg
└── wip-drafts/               # Unnumbered (hidden from nav)
    └── 001-test.jpg
```

## Usage

Tests copy this to a temp directory and run the scanner against it.

The `.jpg` files here are 1x1 pixel placeholders—real image processing tests
use the actual images in `001-NY/`.
