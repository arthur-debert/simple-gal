# simple-gal

A static site generator for fine art photography portfolios. Your filesystem is the data source — no database, no CMS, no lock-in. The output is plain HTML and CSS that will work on any file server, today and twenty years from now.

| Gallery page | Photo page |
| :-: | :-: |
| ![Gallery page](assets/gallery-page.jpeg) | ![Photo page](assets/photo-page.jpeg) |

## Why

Most gallery solutions come with features you didn't ask for (comments, social buttons, login screens), breaking changes you didn't expect, subscription fees for hosting what amounts to static files, and image pipelines you can't control.

Simple Gal takes the opposite approach. You get a single binary that reads a directory of images and produces a self-contained site — about 9 KB per page before images, ~30 lines of vanilla JavaScript (for keyboard shortcuts and swipe gestures; click navigation is pure HTML/CSS), zero runtime dependencies, zero API calls. If JavaScript stopped running entirely, every photo would still be browsable by clicking.

Read more: [The Forever Stack](https://simple-gal.magik.works/philosophy/forever-stack.html) — why longevity drives every design decision.

## Quick start

```bash
cargo install simple-gal          # or grab a binary from Releases
mkdir -p content/010-My-Album
cp ~/Photos/favorites/*.jpg content/010-My-Album/
simple-gal build                  # content/ → dist/
```

`dist/` now contains a complete gallery site. The directory name becomes the album title, the numeric prefix controls navigation order, and the first image is the album's thumbnail.

Read more: [Quick Start guide](https://simple-gal.magik.works/getting-started.html)

## Features

### Filesystem as data source

Directories become albums. Images become photos. No tool-specific data format — your files are the truth.

```text
content/
├── config.toml                   # site-wide config (optional)
├── site.md                       # home page description (optional)
├── 010-Landscapes/               # album
│   ├── description.md            # album description
│   ├── 001-dawn.jpg
│   └── 002-dusk.jpg
├── 020-Travel/                   # group (contains sub-albums)
│   ├── 010-Japan/
│   └── 020-Italy/
└── 040-about.md                  # standalone page
```

Groups generate gallery-list pages showing thumbnail cards for each child album. Nesting goes to any depth.

Read more: [Directory Structure](https://simple-gal.magik.works/content/directory-structure.html) · [Albums and Groups](https://simple-gal.magik.works/content/albums-and-groups.html) · [Ordering and Naming](https://simple-gal.magik.works/content/ordering-and-naming.html)

### Photographic control

You set image quality, sharpening, responsive breakpoints, and thumbnail aspect ratios — per album if you want. AVIF output with content-addressed caching means incremental builds are near-instant.

```toml
# content/020-Travel/010-Japan/config.toml
[images]
quality = 75

[thumbnails]
aspect_ratio = [1, 1]
```

Configuration cascades: stock defaults → root config → group config → album config. Only the keys you specify are overridden.

Read more: [Configuration](https://simple-gal.magik.works/configuration/overview.html) · [Image Quality](https://simple-gal.magik.works/images/quality.html) · [Responsive Sizes](https://simple-gal.magik.works/images/responsive-sizes.html) · [Thumbnails](https://simple-gal.magik.works/images/thumbnails.html) · [Processing Cache](https://simple-gal.magik.works/images/caching.html)

### Customization

Colors, fonts, and theme spacing are set via `config.toml` and exposed as CSS custom properties. For deeper changes, drop files into `assets/`:

- `custom.css` — loaded after the main stylesheet
- `head.html` — injected at end of `<head>` (analytics, meta tags)
- `body-end.html` — injected before `</body>` (tracking scripts)

Read more: [Colors and Theme](https://simple-gal.magik.works/configuration/colors-and-theme.html) · [Fonts](https://simple-gal.magik.works/configuration/fonts.html) · [CSS and JS injection](https://simple-gal.magik.works/customization/css-and-js.html) · [Advanced CSS](https://simple-gal.magik.works/customization/advanced.html)

### Progressive Web App

Every generated site is installable as a home-screen app out of the box — offline-capable, app-like viewing with no extra configuration.

Read more: [PWA](https://simple-gal.magik.works/pwa/overview.html)

### Deployment

The output is a directory of static files. Serve it from anywhere: GitHub Pages, Netlify, S3, Nginx, Apache, a Raspberry Pi.

```bash
simple-gal build --source my-photos --output dist
# deploy dist/ however you like
```

A ready-made GitHub Action handles build + deploy in one step.

Read more: [Deployment](https://simple-gal.magik.works/deployment/local.html) · [GitHub Actions](https://simple-gal.magik.works/deployment/github-actions.html)

## What it does not do

Simple Gal generates a portfolio, not a platform. No comments, no search, no e-commerce, no client proofing, no analytics (unless you inject your own). Each omission removes a dependency that could break. Read [The Forever Stack](https://simple-gal.magik.works/philosophy/forever-stack.html) for the full rationale.

## Documentation

**[simple-gal.magik.works](https://simple-gal.magik.works)** — the full guide.

## Links

- [Changelog](CHANGELOG.md)
- [GitHub Action](https://github.com/arthur-debert/simple-gal-action)
- [Releases](https://github.com/arthur-debert/simple-gal/releases)
