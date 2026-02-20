# Ordering and Naming

Simple Gal uses a numeric prefix convention (`NNN-name`) to control the order of albums, images, and pages. This convention applies uniformly to all entry types: directories, image files, and markdown pages.

## The NNN-prefix convention

Prefix any filename or directory name with a number followed by a dash to set its sort position and make it visible in navigation:

```text
010-Landscapes/
020-Travel/
030-Minimal/
```

The number can be any non-negative integer. Leading zeros are stripped during parsing (`010` becomes `10`). Items are sorted by their numeric value, not lexicographically.

Common numbering patterns:

| Pattern | Use case |
|---------|----------|
| `001`, `002`, `003` | Sequential, no room for insertion |
| `010`, `020`, `030` | Gaps of 10, easy to insert between |
| `100`, `200`, `300` | Large gaps for frequently reorganized content |

## Display titles

The name portion after the numeric prefix becomes the display title. Dashes in the name are converted to spaces:

| Filename | Display title |
|----------|--------------|
| `020-My-Best-Photos/` | My Best Photos |
| `010-Landscapes/` | Landscapes |
| `001-dawn.jpg` | dawn |
| `001-My-Museum.jpg` | My Museum |
| `040-who-am-i.md` | who am i |

This conversion applies to all entry types: albums, images, and pages. For pages, the display title is used as the navigation label (link title). For images, it is used as the photo title in breadcrumbs and image detail pages.

## Entries without a number prefix

Directories and files without a number prefix are still processed, but they are hidden from navigation. They remain accessible via direct URL.

```text
content/
├── 010-Landscapes/    # In nav, accessible at /Landscapes/
├── 020-Travel/        # In nav, accessible at /Travel/
└── wip-drafts/        # NOT in nav, accessible at /wip-drafts/
```

This is useful for:

- Work-in-progress albums you want to preview but not publish in navigation
- Draft pages you want to generate but not link from the site
- Unlisted content shared via direct link

Unnumbered entries still get a display title with dashes converted to spaces (e.g., `wip-drafts` displays as "wip drafts"), but since they have no number prefix, the directory name is used as-is for the album title.

The same rule applies to pages:

```text
content/
├── 010-about.md       # In nav
├── notes.md           # Generated at /notes/ but NOT in nav
```

## Number-only entries

An entry can be just a number with no name:

| Filename | Number | Name | Display title |
|----------|--------|------|---------------|
| `001.jpg` | 1 | (empty) | (none) |
| `001-.jpg` | 1 | (empty) | (none) |

Number-only images have no title and will not display a title in the breadcrumb.

## Thumb convention for album thumbnails

An image whose name starts with `thumb` (case-insensitive) is used as the album's representative thumbnail on the index page:

```text
content/010-Landscapes/
├── 001-dawn.jpg
├── 005-thumb.jpg              # Album thumbnail
└── 010-night.jpg
```

You can add a title after `thumb-`:

```text
005-thumb-The-Sunset.jpg       # Thumb with title "The Sunset"
```

The `thumb` prefix is stripped from the display title — `005-thumb-The-Sunset.jpg` displays as "The Sunset", not "thumb The Sunset". The image still appears normally in the album.

Only one thumb image per album is allowed. Multiple thumb images cause a build error. See [Thumbnails](../images/thumbnails.md) for full details.

## Duplicate numbers are errors

Two images with the same number prefix within the same album will cause a build error:

```text
content/010-Landscapes/
├── 001-dawn.jpg
└── 001-sunset.jpg     # Error: Duplicate image number 1
```

```text
Error: Duplicate image number 1 in content/010-Landscapes
```

Rename one of the files to use a different number:

```text
content/010-Landscapes/
├── 001-dawn.jpg
└── 002-sunset.jpg     # Fixed
```

This rule applies only to images within the same album. Different albums can freely reuse the same numbers.

## How ordering works in practice

Images within an album are sorted by their numeric prefix:

```text
content/010-Landscapes/
├── 001-dawn.jpg       # Displayed first
├── 002-dusk.jpg       # Displayed second
└── 010-night.jpg      # Displayed third
```

Albums and groups are sorted by their directory number:

```text
content/
├── 010-Landscapes/    # First in nav
├── 020-Travel/        # Second in nav
└── 030-Minimal/       # Third in nav
```

Pages are sorted by their file number:

```text
content/
├── 040-about.md       # First page in nav
└── 050-github.md      # Second page in nav
```

Unnumbered images are sorted after all numbered images, preserving filename order among themselves.
