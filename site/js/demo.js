// The demo videos play on their own, muted and in a loop. With reduced motion they stay on their
// posters and show the player controls instead, so they only play when asked.
(function () {
    if (!window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
        return;
    }
    document.querySelectorAll(".demo-video video").forEach(function (video) {
        video.removeAttribute("autoplay");
        video.pause();
        video.controls = true;
    });
})();
