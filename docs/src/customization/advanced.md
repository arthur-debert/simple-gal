# Advanced Customization

This chapter covers the CSS custom properties and class names available for styling overrides. All overrides go in `assets/custom.css` (see [Custom CSS and HTML Snippets](css-and-js.md)).

## CSS Custom Properties

Simple Gal generates CSS custom properties from your `config.toml` values and injects them into a `:root` block before the main stylesheet. You can override any of them in `custom.css`.

### Color Properties

| Property | Default | Controls |
|----------|---------|----------|
| `--color-bg` | `#ffffff` | Page background |
| `--color-text` | `#1a1a1a` | Primary text |
| `--color-text-muted` | `#6b6b6b` | Secondary text (captions, descriptions, nav groups) |
| `--color-border` | `#e0e0e0` | Borders, image placeholder background |
| `--color-link` | `#1a1a1a` | Link text |
| `--color-link-hover` | `#4a4a4a` | Link hover state |
| `--color-separator` | `#e8e8e8` | Header border, nav dividers |

These are best set via `config.toml` under `[colors]`, but you can override them in `custom.css` when you need conditional logic (e.g., dark mode via `prefers-color-scheme`).

### Theme Properties

| Property | Default | Controls |
|----------|---------|----------|
| `--mat-x` | `2rem` | Horizontal mat (padding) around photos on image pages |
| `--mat-y` | `2rem` | Vertical mat (padding) around photos on image pages |
| `--thumbnail-gap` | `0.5rem` | Gap between thumbnails in album and index grids |
| `--grid-padding` | `0` | Outer padding of thumbnail grids |

### Font Properties

| Property | Default | Controls |
|----------|---------|----------|
| `--font-family` | `system-ui, sans-serif` | Font stack for all text |
| `--font-weight` | `400` | Base font weight |

### Internal Properties

These are defined in the stylesheet and not generated from config, but you can still override them:

| Property | Default | Controls |
|----------|---------|----------|
| `--header-height` | `3rem` | Height of the fixed header bar |
| `--font-size-base` | `18px` | Base font size |
| `--font-size-small` | `14px` | Small text (captions, metadata) |
| `--font-size-heading` | `1.5rem` | Album and page headings |
| `--transition-speed` | `0.2s` | Duration of hover transitions |

## Key CSS Classes

These are the main classes in the generated HTML. Target them in `custom.css` for structural overrides.

### Layout

| Class | Element | Description |
|-------|---------|-------------|
| `.site-header` | `<header>` | Fixed top bar with breadcrumb and navigation |
| `.breadcrumb` | `<nav>` | Breadcrumb trail inside the header |
| `.site-nav` | `<nav>` | Navigation container (hamburger menu) |
| `.nav-panel` | `<div>` | Slide-in navigation panel |

### Index Page

| Class | Element | Description |
|-------|---------|-------------|
| `.index-page` | `<main>` | Index page main container |
| `.index-header` | `<div>` | Site title and description block |
| `.album-grid` | `<div>` | Grid of album cards |
| `.album-card` | `<a>` | Individual album link with thumbnail and title |
| `.album-title` | `<span>` | Album title text below thumbnail |

### Album Page

| Class | Element | Description |
|-------|---------|-------------|
| `.album-page` | `<main>` | Album page main container |
| `.album-header` | `<div>` | Album title and description block |
| `.album-description` | `<div>` | Album description text |
| `.thumbnail-grid` | `<div>` | Grid of image thumbnails |
| `.thumb-link` | `<a>` | Individual thumbnail link |

### Image Page

| Class | Element | Description |
|-------|---------|-------------|
| `body.image-view` | `<body>` | Body class on image pages (sets `overflow: hidden`) |
| `.image-page` | `<div>` | Image page main container |
| `.image-frame` | `<div>` | Container for the photo itself |
| `.image-caption` | `<div>` | Short caption below the image |
| `.image-description` | `<div>` | Long description (scrollable) |
| `.image-nav` | `<div>` | Navigation dots between images |
| `.nav-prev`, `.nav-next` | `<a>` | Invisible click zones for prev/next navigation |

### Content Pages

| Class | Element | Description |
|-------|---------|-------------|
| `.page` | `<main>` | Content page main container |
| `.page-content` | `<div>` | Rendered markdown content |

## Examples

### Dark Mode Override

Override colors when the user prefers a dark color scheme:

```css
@media (prefers-color-scheme: dark) {
    :root {
        --color-bg: #1a1a1a;
        --color-text: #e0e0e0;
        --color-text-muted: #999999;
        --color-border: #333333;
        --color-link: #e0e0e0;
        --color-link-hover: #ffffff;
        --color-separator: #333333;
    }
}
```

### Larger Thumbnails

Make album cards bigger on the index page:

```css
.album-grid {
    grid-template-columns: repeat(auto-fill, minmax(400px, 1fr));
}
```

### Tighter Thumbnail Grid

Remove gaps for a mosaic look:

```css
:root {
    --thumbnail-gap: 0;
    --grid-padding: 0;
}
```

### More Mat Space

Give photos more breathing room on image pages:

```css
:root {
    --mat-x: 6rem;
    --mat-y: 4rem;
}
```

### Custom Hover Effect

Replace the default subtle zoom with an opacity fade:

```css
.album-card:hover img {
    transform: none;
    opacity: 0.8;
}

.thumb-link:hover img {
    transform: none;
    opacity: 0.85;
}
```

### Hide the Header

Remove the fixed header entirely:

```css
.site-header {
    display: none;
}

main {
    margin-top: 0;
}
```

### Adjust Header Height

Make the header taller and change its font size:

```css
:root {
    --header-height: 4rem;
}

.site-header {
    font-size: 16px;
}
```

### Style Captions

Customize caption appearance on image pages:

```css
.image-caption {
    font-style: italic;
    text-align: center;
    font-size: 13px;
}
```

### Fixed-Column Grid

Use a fixed number of columns instead of auto-fill:

```css
.thumbnail-grid {
    grid-template-columns: repeat(3, 1fr);
}

@media (max-width: 768px) {
    .thumbnail-grid {
        grid-template-columns: repeat(2, 1fr);
    }
}
```

### Square Thumbnails

Change the default 4:5 aspect ratio to 1:1:

```css
.album-card img,
.thumb-link img {
    aspect-ratio: 1 / 1;
}
```

## Tips

- Override custom properties on `:root` to change values globally. Override them on specific selectors for scoped changes.
- `custom.css` loads after the main stylesheet, so your rules win at equal specificity. You should rarely need `!important`.
- Use your browser's developer tools to inspect the generated HTML and find the exact class names and structure for the element you want to customize.
- View transitions use a 0.2-second fade by default. Override the `::view-transition-old(root)` and `::view-transition-new(root)` pseudo-elements to change the transition animation.
