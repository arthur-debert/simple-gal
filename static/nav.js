// Simple Gal - Keyboard & Swipe Navigation
(function() {
    var prev = document.querySelector('.nav-prev');
    var next = document.querySelector('.nav-next');
    var prevUrl = prev && prev.getAttribute('href');
    var nextUrl = next && next.getAttribute('href');

    // Parent URL: last <a> in the breadcrumb trail (the level above this page).
    // On the home page the only link is the home link itself — no parent to go to.
    var crumbs = document.querySelectorAll('.breadcrumb a');
    var lastCrumb = crumbs.length > 1 ? crumbs[crumbs.length - 1] : null;
    var parentUrl = lastCrumb ? lastCrumb.getAttribute('href') : null;

    // Position click zones so they overlap ~20% of the image on each side
    // and extend outward to the page edges.
    var frame = document.querySelector('.image-frame');
    if (frame && prev && next) {
        var OVERLAP = 0.2;
        function sizeNavZones() {
            var r = frame.getBoundingClientRect();
            if (r.width === 0) return;
            if (prev) prev.style.width = (r.left + r.width * OVERLAP) + 'px';
            if (next) next.style.width = (window.innerWidth - r.right + r.width * OVERLAP) + 'px';
        }
        sizeNavZones();
        window.addEventListener('resize', sizeNavZones);
    }

    // Keyboard navigation
    document.addEventListener('keydown', function(e) {
        // Previous: ArrowLeft, h, k
        if (e.key === 'ArrowLeft' || e.key === 'h' || e.key === 'k') {
            if (prevUrl) location.href = prevUrl;
        // Next: ArrowRight, l, j
        } else if (e.key === 'ArrowRight' || e.key === 'l' || e.key === 'j') {
            if (nextUrl) location.href = nextUrl;
        // Up a level: ArrowUp, Escape
        } else if (e.key === 'ArrowUp' || e.key === 'Escape') {
            if (parentUrl) location.href = parentUrl;
        }
    });

    // Touch/swipe navigation (image pages only)
    if (prev || next) {
        var sx = 0, sy = 0;
        document.addEventListener('touchstart', function(e) {
            sx = e.touches[0].clientX;
            sy = e.touches[0].clientY;
        }, { passive: true });
        document.addEventListener('touchend', function(e) {
            if (e.target.closest('nav, a, button')) return;
            var dx = e.changedTouches[0].clientX - sx;
            var dy = e.changedTouches[0].clientY - sy;
            if (Math.abs(dx) > Math.abs(dy) && Math.abs(dx) > 50) {
                location.href = dx > 0 ? prevUrl : nextUrl;
            }
        }, { passive: true });
    }
})();
