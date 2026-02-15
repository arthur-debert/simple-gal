# Photo Detail Page — Layout Spec

The photo detail page is the core of the gallery experience. This spec defines
its layout structure, terminology, and the three rendering variants.

## Zones

```
╔══════════════════════════════════════════════╗
║  Site Header                            ☰   ║  fixed top
╠══════════════════════════════════════════════╣
║                                              ║
║              Photo Viewport                  ║  between header and image-nav
║                                              ║
╠══════════════════════════════════════════════╣
║            ● ● ◉ ● ● ●                      ║  sticky bottom
╚══════════════════════════════════════════════╝
```

| Zone | CSS | Role |
|---|---|---|
| **Site Header** | `.site-header` | Fixed top bar. Breadcrumb (left) + hamburger nav (right). |
| **Photo Viewport** | `body.image-view main` | The gallery wall — everything between header and image-nav. |
| **Image Nav** | `.image-nav` | Sticky bottom bar. Dot per image, current highlighted. |

Invisible click zones (`.nav-prev`, `.nav-next`) cover the left/right 30% of
the viewport for prev/next navigation. They overlay everything and are not part
of the visual layout.

## Photo Viewport anatomy

The Photo Viewport contains a **Matted Photo** and optionally a **Description**.

### Mat

User-controlled whitespace surrounding the photo, configured via `mat_x` /
`mat_y` in `config.toml` (CSS: `--mat-x`, `--mat-y`). This is the
breathing room that frames the photo — analogous to a gallery mat board.

**The mat is sacred.** Its dimensions never change between layout variants.
The photo shrinks to accommodate captions or description teasers, but the outer
mat boundary stays fixed.

### Matted Photo

The photo (and optional caption) centered within the mat:

- **Photo** (`.image-frame`): Aspect-ratio constrained image. Fills the
  maximum area within the mat while preserving its ratio.
- **Caption** (`.image-caption`): Short text (≤160 chars). Flush below the
  photo, no gap. Part of the matted presentation — lives inside the mat.
  The photo shrinks to make room for the caption.

### Description

Long text (>160 chars). Lives **below** the mat, not part of the framed
presentation. When present, a 1–2 line teaser peeks above the Image Nav to
signal that more content exists. The photo shrinks slightly to guarantee the
teaser is visible, while the mat dimensions remain unchanged.

### Caption vs Description

Both originate from the same source (IPTC caption or sidecar file). They are
**mutually exclusive** — split by length:

- ≤160 chars → **caption** (bound to the photo, inside the mat)
- \>160 chars → **description** (below the mat, scrollable)

A given image has one, the other, or neither. Never both.

## Three layout variants

### A. Photo only

No text metadata. Photo centered in mat, no scroll.

```
╠══════════════════════════════════════════════╣
║                                              ║
║            ┊  top mat  ┊                     ║
║            ┊           ┊                     ║
║   left ┌───────────────────┐  right          ║
║   mat  │                   │  mat            ║
║        │      Photo        │                 ║
║        │                   │                 ║
║   left └───────────────────┘  right          ║
║   mat                         mat            ║
║            ┊           ┊                     ║
║            ┊ bottom mat┊                     ║
║                                              ║
╠══════════════════════════════════════════════╣
║            ● ● ◉ ● ● ●                      ║
╚══════════════════════════════════════════════╝
```

### B. Photo + Caption (≤160 chars)

Caption is flush below the photo, inside the mat. Photo shrinks to make room;
mat boundary unchanged. No scroll.

```
╠══════════════════════════════════════════════╣
║                                              ║
║            ┊  top mat  ┊                     ║
║            ┊           ┊                     ║
║        ┌───────────────────┐                 ║
║        │                   │                 ║
║        │   Photo (smaller) │                 ║
║        │                   │                 ║
║        └───────────────────┘                 ║
║        A quiet morning...    ← caption       ║
║            ┊           ┊     (same width     ║
║            ┊ bottom mat┊      as photo)      ║
║                                              ║
╠══════════════════════════════════════════════╣
║            ● ● ◉ ● ● ●                      ║
╚══════════════════════════════════════════════╝
```

### C. Photo + Description (>160 chars)

Description lives below the mat. Photo shrinks slightly (via `--desc-peek`)
to guarantee the teaser is visible; mat dimensions unchanged. The entire
content (matted photo + description) scrolls as a unit; Image Nav stays
sticky at the bottom.

```
╠══════════════════════════════════════════════╣
║                                              ║  ↑
║            ┊  top mat  ┊                     ║  │
║        ┌───────────────────┐                 ║  │
║        │                   │                 ║  │ scrolls
║        │  Photo (smaller)  │                 ║  │ as one
║        │                   │                 ║  │ unit
║        └───────────────────┘                 ║  │
║            ┊           ┊                     ║  │
║            ┊ bottom mat┊                     ║  │
║                                              ║  │
║        This photograph was taken on a   ← teaser
║        cold February morning in...        (1-2 lines)
╠══════════════════════════════════════════════╣
║            ● ● ◉ ● ● ●                      ║  Image Nav (sticky)
╚══════════════════════════════════════════════╝

                 ↓ scroll ↓

╠══════════════════════════════════════════════╣
║        This photograph was taken on a        ║
║        cold February morning in Patagonia.   ║
║        The light was extraordinary — a pale  ║  Description
║        gold washing across the glacial       ║  (max-width: 65ch)
║        lake while the peaks caught the       ║
║        first rays of sun...                  ║
╠══════════════════════════════════════════════╣
║            ● ● ◉ ● ● ●                      ║  stays pinned
╚══════════════════════════════════════════════╝
```

## CSS class → spec mapping

| CSS class | Spec term | Notes |
|---|---|---|
| `.site-header` | Site Header | |
| `.image-nav` | Image Nav | |
| `body.image-view main` | Photo Viewport | |
| `--mat-x`, `--mat-y` | Mat | User-controlled whitespace around the photo |
| `.image-frame` | Photo | Aspect-ratio constrained container |
| `.image-caption` | Caption | ≤160 chars, inside mat |
| `.image-description` | Description | >160 chars, below mat |
| `.image-page` | (container) | Wraps Photo + Caption; sizing target for container queries |
| `.nav-prev`, `.nav-next` | (interaction) | Invisible click zones, not visual layout |
