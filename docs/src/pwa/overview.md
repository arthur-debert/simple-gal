# PWA Overview

Every site Simple Gal generates is a Progressive Web App. There is nothing to enable -- it works out of the box.

## What this means for your visitors

When someone visits your portfolio on their phone, their browser will offer to install it as an app. Once installed, your portfolio:

- **Lives on their home screen** alongside their other apps, with your own icon and name.
- **Opens full-screen** without browser chrome -- no address bar, no tabs, just your photos.
- **Loads instantly on repeat visits.** A service worker caches your pages and images, so returning visitors see content immediately even on slow connections.
- **Works offline.** Pages and images that have been viewed before are available without a network connection. A visitor who browsed your Japan album on hotel wifi can revisit those photos on the plane.

None of this requires your visitors to visit an app store or create an account. The browser handles everything.

## How it works (briefly)

Simple Gal generates three things that make this possible:

1. **A service worker** (`sw.js`) -- a small script that runs in the background and manages a local cache of your site's pages and images.
2. **A web manifest** (`site.webmanifest`) -- a file that tells the browser your site's name, icons, and display preferences.
3. **Registration code** on every page that connects the two.

The caching strategy is stale-while-revalidate: the service worker serves cached content immediately, then fetches a fresh copy in the background. This means returning visitors always get a fast response, and the cache quietly stays up to date.

## Automatic cache management

Each build stamps the cache with a version identifier (`simple-gal-v<version>`). When you deploy an updated site, the new service worker activates and cleans up old caches automatically. Your visitors get the new content on their next visit without any stale-cache issues.

## Zero configuration

The PWA works with stock defaults and no configuration. If you want to customize the app name, icons, or theme color, see [Customizing](customizing.md). But it is entirely optional -- the default setup is a fully functional PWA.
