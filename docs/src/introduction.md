# Introduction

Simple Gal is a static site generator built for photographers who want a portfolio that lasts.

You point it at a directory of images, run one command, and get a complete gallery site -- HTML, CSS, responsive images, and nothing else. No database. No CMS. No JavaScript framework. The output can be dropped onto any file server and it will work, today and twenty years from now.

## Who it's for

Simple Gal is for photographers -- professional or otherwise -- who have accumulated galleries over time and want a clean, permanent home for them online. If you care about how your images are presented (compression, sharpening, aspect ratios, ordering) and you don't want to fight a platform to get it right, this is for you.

It is not for photographers who need comments, search, e-commerce, or client proofing. It generates a portfolio, not a platform.

## The problem

Most gallery solutions share the same failure modes:

- **Features you didn't ask for.** Login screens, likes, comments, social sharing buttons -- bolted on for the platform's benefit, not yours.
- **Breaking changes.** Updates that rearrange your layout, change your URLs, or require migration work.
- **Subscription fees.** Monthly charges for hosting what amounts to static files.
- **Server infrastructure.** Maintenance, updates, security patches, database backups -- all for showing pictures.
- **Bespoke data models.** Your images and metadata locked into a format that forces you to start over when you switch tools.
- **Poor photographic control.** Aggressive compression, generic center-crops, no control over sharpening or aspect ratios.

These aren't edge cases. They're the norm. If you've been making photographs for a decade, you've probably been through several of these cycles already.

## What Simple Gal gives you

**Your filesystem is the data source.** Directories become albums. Images become photos. Filenames and IPTC tags provide titles and descriptions. There is no tool-specific data format to migrate from -- your files are the truth.

**Photographic control.** You set the image quality, sharpening, responsive breakpoints, and thumbnail aspect ratios. Per gallery, if you want. No more fighting a platform's one-size-fits-all image pipeline.

**Fast, clean output.** Each page weighs about 9 KB before images. Navigation is page-based with smooth transitions. Mobile-first layout with swipe support. Light and dark modes. No spinners, no layout shift, no pop-ups.

**Nothing to maintain.** The generated site is plain HTML and CSS with about 30 lines of vanilla JavaScript (for keyboard shortcuts and swipe gestures -- click navigation is pure HTML/CSS). No runtime dependencies. No build tools. No framework updates to chase.

**Installable.** Every generated site is a Progressive Web App out of the box. Visitors can add it to their home screen for offline, app-like viewing.

## How it works

Simple Gal runs a three-stage pipeline:

1. **Scan** -- reads your content directory and builds a manifest of albums, images, pages, and configuration.
2. **Process** -- generates responsive image sizes and thumbnails in AVIF format.
3. **Generate** -- produces the final HTML site with inline CSS.

You run it with a single command:

```bash
simple-gal build
```

The output lands in `dist/` and is ready to deploy. See the [Quick Start](getting-started.md) to get a working gallery in five minutes, or read about [The Forever Stack](philosophy/forever-stack.md) to understand why it's built this way.
