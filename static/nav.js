// Simple Gal - Keyboard & Swipe Navigation
(function() {
    var prev = document.querySelector('.nav-prev');
    var next = document.querySelector('.nav-next');
    if (!prev && !next) return;
    var prevUrl = prev && prev.getAttribute('href');
    var nextUrl = next && next.getAttribute('href');

    // Keyboard navigation
    document.addEventListener('keydown', function(e) {
        if (e.key === 'ArrowLeft' || e.key === 'h') {
            if (prevUrl) location.href = prevUrl;
        } else if (e.key === 'ArrowRight' || e.key === 'l') {
            if (nextUrl) location.href = nextUrl;
        } else if (e.key === 'Escape') {
            location.href = '../';
        }
    });

    // Touch/swipe navigation
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
})();
