// Simple Gal - Image Navigation
(function() {
    const zones = document.querySelector('.nav-zones');
    if (!zones) return;

    const prevUrl = zones.dataset.prev;
    const nextUrl = zones.dataset.next;

    // Apply navigation direction class for view transitions
    const navDirection = sessionStorage.getItem('nav-direction');
    if (navDirection) {
        document.documentElement.classList.add('nav-' + navDirection);
        sessionStorage.removeItem('nav-direction');
    }

    // Click navigation
    document.addEventListener('click', function(e) {
        // Ignore clicks on nav, links, etc.
        if (e.target.closest('nav, a, button')) return;

        const x = e.clientX / window.innerWidth;
        if (x < 0.3) {
            navigate(prevUrl, 'back');
        } else if (x > 0.7) {
            navigate(nextUrl, 'forward');
        }
    });

    // Keyboard navigation
    document.addEventListener('keydown', function(e) {
        if (e.key === 'ArrowLeft' || e.key === 'h') {
            navigate(prevUrl, 'back');
        } else if (e.key === 'ArrowRight' || e.key === 'l') {
            navigate(nextUrl, 'forward');
        } else if (e.key === 'Escape') {
            navigate('index.html', 'back');
        }
    });

    // Touch/swipe navigation
    let touchStartX = 0;
    let touchStartY = 0;

    document.addEventListener('touchstart', function(e) {
        touchStartX = e.touches[0].clientX;
        touchStartY = e.touches[0].clientY;
    }, { passive: true });

    document.addEventListener('touchend', function(e) {
        if (e.target.closest('nav, a, button')) return;

        const touchEndX = e.changedTouches[0].clientX;
        const touchEndY = e.changedTouches[0].clientY;
        const deltaX = touchEndX - touchStartX;
        const deltaY = touchEndY - touchStartY;

        // Only trigger if horizontal swipe is dominant
        if (Math.abs(deltaX) > Math.abs(deltaY) && Math.abs(deltaX) > 50) {
            if (deltaX > 0) {
                navigate(prevUrl, 'back');
            } else {
                navigate(nextUrl, 'forward');
            }
        }
    }, { passive: true });

    // Preload adjacent images
    function preload(url) {
        if (url && url !== 'index.html') {
            const link = document.createElement('link');
            link.rel = 'prefetch';
            link.href = url;
            document.head.appendChild(link);
        }
    }

    preload(prevUrl);
    preload(nextUrl);

    function navigate(url, direction) {
        if (url) {
            // Store direction for the next page to pick up
            if (direction) {
                sessionStorage.setItem('nav-direction', direction);
            }
            window.location.href = url;
        }
    }
})();
