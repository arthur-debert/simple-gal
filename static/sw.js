const CACHE_NAME = 'simple-gal-v1';
const IMAGE_CACHE_NAME = CACHE_NAME + '-images';
const MAX_CACHED_IMAGES = 200;
const ASSETS_TO_CACHE = [
    '/',
    '/index.html',
    '/offline.html',
    '/site.webmanifest',
    '/icon-192.png',
    '/icon-512.png',
    '/apple-touch-icon.png'
];

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
    const keep = new Set([CACHE_NAME, IMAGE_CACHE_NAME]);
    event.waitUntil(
        caches.keys().then((cacheNames) => {
            return Promise.all(
                cacheNames
                    .filter((name) => !keep.has(name))
                    .map((name) => caches.delete(name))
            );
        })
    );
});

// Fetch event: route by request type
self.addEventListener('fetch', (event) => {
    const url = new URL(event.request.url);

    // Ignore cross-origin requests (analytics, fonts, etc.)
    if (url.origin !== location.origin) return;

    // Navigation requests (HTML) - Network First, fall back to cache
    if (event.request.mode === 'navigate') {
        event.respondWith(
            fetch(event.request)
                .then((response) => {
                    if (response.ok) {
                        const cloned = response.clone();
                        caches.open(CACHE_NAME).then((cache) => {
                            cache.put(event.request, cloned);
                        });
                    }
                    return response;
                })
                .catch(() => {
                    return caches.match(event.request).then((cached) => {
                        return cached || caches.match('/offline.html');
                    });
                })
                .then((response) => {
                    return response || new Response('Offline', {
                        status: 503,
                        headers: { 'Content-Type': 'text/plain' },
                    });
                })
        );
        return;
    }

    // Image requests - Cache First, fall back to network
    // Uses a separate bounded cache (MAX_CACHED_IMAGES) to prevent unbounded storage growth.
    // When the limit is exceeded, the oldest entries are evicted (FIFO).
    if (event.request.destination === 'image') {
        event.respondWith(
            caches.open(IMAGE_CACHE_NAME).then((cache) => {
                return cache.match(event.request).then((cachedResponse) => {
                    if (cachedResponse) {
                        return cachedResponse;
                    }
                    return fetch(event.request).then((response) => {
                        if (response.ok) {
                            cache.put(event.request, response.clone());
                            cache.keys().then((keys) => {
                                if (keys.length > MAX_CACHED_IMAGES) {
                                    cache.delete(keys[0]);
                                }
                            });
                        }
                        return response;
                    });
                });
            }).catch(() => {
                return new Response('', { status: 504 });
            })
        );
        return;
    }

    // Default: Stale-While-Revalidate
    event.respondWith(
        caches.match(event.request).then((cachedResponse) => {
            const fetchPromise = fetch(event.request)
                .then((networkResponse) => {
                    if (networkResponse.ok) {
                        const cloned = networkResponse.clone();
                        caches.open(CACHE_NAME).then((cache) => {
                            cache.put(event.request, cloned);
                        });
                    }
                    return networkResponse;
                })
                .catch(() => {
                    return cachedResponse || new Response('', {
                        status: 504,
                        headers: { 'Content-Type': 'text/plain' },
                    });
                });
            return cachedResponse || fetchPromise;
        })
    );
});
