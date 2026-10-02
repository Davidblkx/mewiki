"use strict";

if ("serviceWorker" in navigator) {
    navigator.serviceWorker.register("/sw.js").catch(() => {
        // Service workers need HTTPS outside localhost; the wiki still works online without one.
    });
}
