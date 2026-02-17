# Custom CSS and HTML Snippets

Simple Gal supports three **convention files** that let you inject custom CSS, analytics, tracking scripts, and other HTML into every page. Drop them into your `assets/` directory and they are picked up automatically -- no configuration required.

## Convention Files

| File | Injection point | Typical use |
|------|----------------|-------------|
| `custom.css` | `<link>` after the main stylesheet | CSS overrides, layout tweaks, custom fonts |
| `head.html` | End of `<head>`, after all other tags | Analytics snippets, Open Graph meta tags, additional `<link>` or `<meta>` elements |
| `body-end.html` | Immediately before `</body>` | Tracking scripts, chat widgets, cookie banners |

All three are optional. Use any combination you need.

## How It Works

During the build, Simple Gal copies everything in `assets/` to the output root. It then checks whether `custom.css`, `head.html`, or `body-end.html` exist in the output directory and injects references or content into every generated HTML page.

- **`custom.css`** is loaded via a `<link rel="stylesheet">` tag that appears *after* the main inline `<style>` block. This means your rules override the defaults at equal specificity -- no `!important` needed.
- **`head.html`** is inserted as raw HTML at the very end of `<head>`, after the service worker script. Use it for anything that belongs in the document head.
- **`body-end.html`** is inserted as raw HTML right before the closing `</body>` tag, after all page content.

## Examples

### Plausible Analytics

Create `assets/head.html`:

```html
<script defer data-domain="yourdomain.com"
        src="https://plausible.io/js/script.js"></script>
```

### Google Analytics

Create `assets/head.html`:

```html
<script async src="https://www.googletagmanager.com/gtag/js?id=G-XXXXXXXXXX"></script>
<script>
  window.dataLayer = window.dataLayer || [];
  function gtag(){dataLayer.push(arguments);}
  gtag('js', new Date());
  gtag('config', 'G-XXXXXXXXXX');
</script>
```

### Open Graph Meta Tags

Add these to `assets/head.html` (you can combine multiple snippets in one file):

```html
<meta property="og:title" content="Jane Doe Photography">
<meta property="og:description" content="Fine art landscape portfolio">
<meta property="og:image" content="https://example.com/og-image.jpg">
<meta property="og:url" content="https://example.com">
```

### CSS Overrides

Create `assets/custom.css`:

```css
/* Increase thumbnail size on the index page */
.album-grid {
    grid-template-columns: repeat(auto-fill, minmax(350px, 1fr));
}

/* Wider mat spacing on photo pages */
:root {
    --mat-x: 4rem;
    --mat-y: 3rem;
}
```

Because `custom.css` loads after the main styles, these rules take effect without needing `!important`.

### Cookie Consent Banner

Create `assets/body-end.html`:

```html
<script defer src="https://cdn.example.com/cookie-consent.js"></script>
<div id="cookie-banner" style="display:none;">
  <!-- your cookie banner markup -->
</div>
```

### Chat Widget

Create `assets/body-end.html`:

```html
<script>
  (function() {
    var s = document.createElement('script');
    s.src = 'https://chat.example.com/widget.js';
    s.async = true;
    document.body.appendChild(s);
  })();
</script>
```

## Directory Layout

A typical setup with all three convention files:

```
my-portfolio/
  config.toml
  assets/
    custom.css          # CSS overrides
    head.html           # Analytics, meta tags
    body-end.html       # Tracking scripts
    fonts/              # Local font files (see Assets chapter)
    favicon.svg         # Custom favicon (see Assets chapter)
  01-landscapes/
    photo-01.jpg
    ...
```

## Tips

- The files are injected **as-is** with no processing or escaping. Make sure your HTML is valid.
- `custom.css` is served as a separate file (not inlined), so the browser can cache it independently.
- You can reference other files in your `assets/` directory from these snippets using absolute paths (e.g., `/fonts/my-font.woff2`), since the entire `assets/` directory is copied to the output root.
- If you only need to change colors, fonts, or spacing, use `config.toml` instead. Convention files are for customizations that go beyond what the configuration system provides.
