# Albums and Groups

Simple Gal organizes content into two types of directories: **albums** (which contain images) and **groups** (which contain other directories). This distinction is automatic -- the scanner looks at what a directory contains and classifies it accordingly.

## Albums

An album is any directory that contains image files. It generates a gallery page with a thumbnail grid and individual photo detail pages.

```text
content/010-Landscapes/
├── 001-dawn.jpg
├── 002-dusk.jpg
└── 010-night.jpg
```

This produces:

- A gallery page at `/Landscapes/` with thumbnails for all three images
- Individual photo pages for each image (e.g., `/Landscapes/1-dawn/`)

### Preview image

Each album has a preview image used as its thumbnail on parent pages (the home page or a group page). The preview is selected as follows:

1. The image with number `001` (i.e., number prefix value 1), if it exists
2. Otherwise, the image with the lowest number prefix

```text
content/010-Landscapes/
├── 001-dawn.jpg       # This is the preview (number 1)
├── 002-dusk.jpg
└── 010-night.jpg
```

```text
content/020-Abstract/
├── 005-first.jpg      # This is the preview (lowest number, no 001)
└── 010-second.jpg
```

Choose your `001` image deliberately -- it represents the album everywhere on the site.

### Album descriptions

An album can have a description displayed above its thumbnail grid. Place a `description.md` or `description.txt` file in the album directory:

```text
content/010-Landscapes/
├── description.md     # or description.txt
├── 001-dawn.jpg
└── ...
```

If both files exist, `description.md` takes priority and `description.txt` is ignored.

**`description.md`** is rendered as Markdown:

```markdown
A week in **Tokyo** and Kyoto -- street photography and temple gardens.
```

**`description.txt`** is plain text with automatic paragraph handling:

```text
A collection of landscape photographs from various locations.

Visit https://example.com for more.
```

Double newlines become `<p>` elements. URLs are automatically linked. HTML characters are escaped.

See [Metadata](metadata.md) for full details on description formatting.

## Groups

A group is a directory that contains subdirectories instead of images. It acts as a container in the navigation hierarchy.

```text
content/020-Travel/
├── 010-Japan/
│   ├── 001-tokyo.jpg
│   └── 002-kyoto.jpg
└── 020-Italy/
    └── 001-rome.jpg
```

`020-Travel/` is a group. It appears in navigation as a clickable "Travel" link with children "Japan" and "Italy". Clicking it navigates to a gallery-list page at `/Travel/` showing thumbnail cards for each child album or sub-group.

Groups can be nested to any depth:

```text
content/010-Work/
├── 010-Commercial/
│   ├── 010-Fashion/
│   │   └── 001-photo.jpg
│   └── 020-Product/
│       └── 001-photo.jpg
└── 020-Editorial/
    └── 001-photo.jpg
```

### Group descriptions

Like albums, groups can have a `description.md` or `description.txt` file. The description is rendered on the group's gallery-list page above the thumbnail grid.

### Group configuration

Groups can have their own `config.toml` that applies to all albums beneath them. Configuration cascades through the hierarchy:

```text
content/
├── config.toml              # Root config
├── 020-Travel/
│   ├── config.toml          # Overrides root for all Travel albums
│   ├── 010-Japan/
│   │   ├── config.toml      # Overrides Travel config for Japan only
│   │   └── 001-tokyo.jpg
│   └── 020-Italy/
│       └── 001-rome.jpg     # Gets Travel config (no local override)
```

The merge chain is: **stock defaults** -> **root config** -> **group config** -> **album config**. Each level overrides only the keys it specifies; everything else is inherited.

For example, if the root sets `quality = 85`, the Travel group sets `aspect_ratio = [1, 1]`, and the Japan album sets `quality = 70`:

- Japan gets: `quality = 70` (own), `aspect_ratio = [1, 1]` (group)
- Italy gets: `quality = 85` (root), `aspect_ratio = [1, 1]` (group)

## The no-mixing rule

A directory cannot contain both images and subdirectories. This is a hard error:

```text
content/010-Mixed/
├── 001-photo.jpg      # Image
└── sub-album/         # Subdirectory
    └── 001-other.jpg
```

```text
Error: Directory contains both images and subdirectories: content/010-Mixed
```

This rule exists because a directory is either an album (displayed as a gallery) or a group (a navigation container). Mixing the two would create ambiguity about how to render the directory.

To fix this, restructure the content so images and subdirectories live in separate directories:

```text
content/010-Mixed/
├── 010-Main/              # Album with the images
│   └── 001-photo.jpg
└── 020-Sub-Album/         # Another album
    └── 001-other.jpg
```

## Navigation structure

The navigation tree is built from numbered directories:

```text
content/
├── 010-Landscapes/           # Nav: "Landscapes"
├── 020-Travel/               # Nav: "Travel" (group)
│   ├── 010-Japan/            #   Nav: "Japan" (child)
│   └── 020-Italy/            #   Nav: "Italy" (child)
├── 030-Minimal/              # Nav: "Minimal"
└── wip-drafts/               # NOT in nav (unnumbered)
```

This produces the navigation:

```text
Landscapes
Travel
  Japan
  Italy
Minimal
```

Unnumbered directories are excluded from the navigation tree entirely. Their albums are still generated and accessible by URL, but no navigation link points to them.

If an unnumbered directory is a group, its children are promoted -- they appear at the parent level rather than being hidden:

```text
content/
├── some-extra/               # Unnumbered group
│   ├── 010-Alpha/            # Promoted to root level in nav
│   └── 020-Beta/             # Promoted to root level in nav
```
