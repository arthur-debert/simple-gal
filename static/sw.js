const CACHE_NAME = 'simple-gal-v1';
const ASSETS_TO_CACHE = [
    '/',
    '/index.html',
    '/site.webmanifest',
    '/icon-192.png',
    '/icon-512.png',
    '/apple-touch-icon.png'
];

// Note: This service worker uses a Cache-First strategy for images.
// Currently, there is no eviction policy (LRU) implemented for the image cache.
// On devices with limited storage, this could potentially grow large if the user
// browses thousands of images. The browser's storage quota management will eventually
// evict the origin's data if space is needed.
//
// Future improvement: Implement a bounded cache for images (e.g. keep last 50).

// Install event: cache core assets
self.addEventListener('install', (event) => {
    event.waitUntil(
        caches.open(CACHE_NAME).then((cache) => {
            return cache.addAll(ASSETS_TO_CACHE);
        })
    );
});

// Activate event: clean up old caches
self.addEventListener('activate', (event) => {
    event.waitUntil(
        caches.keys().then((cacheNames) => {
            return Promise.all(
                cacheNames.map((cacheName) => {
                    if (cacheName !== CACHE_NAME) {
                        return caches.delete(cacheName);
                    }
                })
            );
        })
    );
});

// Fetch event: Network first for HTML, Cache first for others
self.addEventListener('fetch', (event) => {
    const url = new URL(event.request.url);

    // Navigation requests (HTML) - Network First, fall back to cache
    if (event.request.mode === 'navigate') {
        event.respondWith(
            fetch(event.request)
                .then((response) => {
                    return caches.open(CACHE_NAME).then((cache) => {
                        cache.put(event.request, response.clone());
                        return response;
                    });
                })
                .catch(() => {
                    return caches.match(event.request);
                })
        );
        return;
    }

    // Image requests - Cache First, fall back to network
    if (event.request.destination === 'image') {
        event.respondWith(
            caches.match(event.request).then((cachedResponse) => {
                if (cachedResponse) {
                    return cachedResponse;
                }
                return fetch(event.request).then((response) => {
                    return caches.open(CACHE_NAME).then((cache) => {
                        cache.put(event.request, response.clone());
                        return response;
                    });
                });
            })
        );
        return;
    }

    // Default: Stale-While-Revalidate
    event.respondWith(
        caches.match(event.request).then((cachedResponse) => {
            const fetchPromise = fetch(event.request).then((networkResponse) => {
                caches.open(CACHE_NAME).then((cache) => {
                    cache.put(event.request, networkResponse.clone());
                });
                return networkResponse;
            });
            return cachedResponse || fetchPromise;
        })
    );
});
