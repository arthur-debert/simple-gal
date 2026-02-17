# Customizing the PWA

The PWA works without any configuration, but you can control the app name, icons, and theme color.

## App name

The PWA uses `site_title` from your `config.toml` as the app name. This is the name visitors see when they install the app and on their home screen.

```toml
# content/config.toml
site_title = "Sarah Chen Photography"
```

If no `site_title` is set, the PWA falls back to the stock default.

## Icons

Place icon files in your assets directory to replace the defaults:

```text
content/
└── assets/
    ├── icon-192.png          # 192x192 -- used on Android home screens
    ├── icon-512.png          # 512x512 -- used for splash screens
    └── apple-touch-icon.png  # 180x180 -- used on iOS home screens
```

All three files are optional. Any icon you do not provide will use the built-in default.

**Recommendations:**

- Use square PNG images with no transparency for best results across platforms.
- The `icon-192.png` is the most visible -- it is the app icon on most Android devices.
- The `apple-touch-icon.png` is what iOS uses on the home screen. If you provide only one custom icon, make it this one and `icon-192.png`.

## Theme color

The theme color controls the browser toolbar tint and splash screen background on Android. It defaults to white (`#ffffff`).

To change it, place a custom `site.webmanifest` file in your assets directory:

```text
content/
└── assets/
    └── site.webmanifest
```

The file should contain valid JSON. Here is a minimal example that changes only the theme color:

```json
{
  "name": "Sarah Chen Photography",
  "short_name": "Sarah Chen",
  "start_url": "/",
  "display": "standalone",
  "background_color": "#1a1a1a",
  "theme_color": "#1a1a1a",
  "icons": [
    { "src": "/icon-192.png", "sizes": "192x192", "type": "image/png" },
    { "src": "/icon-512.png", "sizes": "512x512", "type": "image/png" }
  ]
}
```

When you provide a custom `site.webmanifest`, it replaces the generated one entirely. Make sure to include all required fields.

## Root-only limitation

The PWA service worker requires that your site is served from the root of its domain. This means:

| URL | Works |
|-----|-------|
| `https://photos.example.com/` | Yes |
| `https://example.com/` | Yes |
| `https://example.com/photos/` | No |

If your portfolio lives under a subdirectory, the service worker will not register correctly and PWA features (offline access, home screen installation) will not work. The site will still function normally as a website -- only the PWA features are affected.

The simplest solution is to use a subdomain (`photos.example.com`) instead of a subdirectory (`example.com/photos/`).
