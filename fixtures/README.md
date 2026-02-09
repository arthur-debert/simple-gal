# Test Fixtures

Shared fixture data for the test suite. Designed to exercise the full
feature set with minimal data — every file is here for a reason.

## `content/` — Primary Fixture

Used by `scan.rs` tests (copied to a temp dir via `setup_fixtures()`).

```
content/
├── config.toml                    # Root config — overrides ALL defaults
├── 010-Landscapes/                # Numbered album (in nav)
│   ├── config.toml                # Per-gallery config override (quality, aspect_ratio)
│   ├── description.txt            # Album description (plain text)
│   ├── 001-dawn.jpg               # Image with sidecar
│   ├── 001-dawn.txt               # Image sidecar (description)
│   ├── 002-dusk.jpg               # Image without sidecar
│   └── 010-night.jpg              # Non-contiguous numbering
├── 020-Travel/                    # Album group (nested galleries)
│   ├── 010-Japan/                 # Nested album
│   │   ├── description.txt        # Plain text description (lower priority)
│   │   ├── description.md         # Markdown description (takes priority over .txt)
│   │   ├── 001-tokyo.jpg          # Image with sidecar
│   │   └── 001-tokyo.txt          # Image sidecar
│   └── 020-Italy/
│       └── 001-rome.jpg           # No description, no sidecar
├── 030-Minimal/                   # Album with single image, no extras
│   └── 001-solo.jpg
├── 040-about.md                   # Page (numbered, in nav, has # heading)
├── 050-github.md                  # Link page (single URL as body)
└── wip-drafts/                    # Unnumbered (hidden from nav)
    └── 001-test.jpg
```

### What each piece exercises

| Fixture element | Feature tested |
|---|---|
| Root `config.toml` (all keys) | Config loading picks up every field, not just defaults |
| `010-Landscapes/config.toml` | Per-gallery config overrides root; config chain merging |
| `description.txt` (Landscapes) | Album description from plain text (paragraphs, linkification) |
| `description.md` + `description.txt` (Japan) | Markdown takes priority over plain text |
| `001-dawn.txt`, `001-tokyo.txt` | Image sidecar descriptions |
| `002-dusk.jpg` (no sidecar) | Images without sidecars get no description |
| `020-Travel/` with nested albums | Album groups, nested navigation |
| `030-Minimal/` | Single-image album, no description, no config |
| `040-about.md` | Page with heading (title extraction), in-nav |
| `050-github.md` | Link page detection (single URL body) |
| `wip-drafts/` | Unnumbered directory hidden from nav |

### Image files

The `.jpg` files are 1x1 pixel placeholders. Image processing tests
use their own test images — these fixtures are for scan/config/structure testing.

## `browser-content/` — Browser Layout Fixture

Used by `tests/browser_layout.rs` (headless Chrome). Separate from
`content/` because browser tests need specific image dimensions and
description lengths for layout assertions.
