"use strict";

const syncButton = document.getElementById("sync");
const syncStatus = document.getElementById("sync-status");

if (!("caches" in window)) {
    syncButton.disabled = true;
    syncStatus.textContent = "Offline reading needs HTTPS.";
}

syncButton.addEventListener("click", async () => {
    syncButton.disabled = true;
    try {
        const listing = await fetch("/_/api/pages");
        if (!listing.ok) throw new Error(`the page list failed (${listing.status})`);
        const urls = [...(await listing.json()).map((page) => page.url), "/_/custom.css"];
        const cache = await caches.open("pages");
        let saved = 0;
        let failed = 0;
        for (const url of urls) {
            syncStatus.textContent = `Saving ${saved + failed + 1} of ${urls.length}…`;
            try {
                const response = await fetch(url);
                if (response.ok) {
                    await cache.put(url, response);
                    saved += 1;
                } else {
                    failed += 1;
                }
            } catch {
                failed += 1;
            }
        }
        syncStatus.textContent = failed
            ? `Saved ${saved} of ${urls.length}; ${failed} failed.`
            : `Saved all ${saved} for offline reading.`;
    } catch (error) {
        syncStatus.textContent = `Sync failed: ${error.message}.`;
    } finally {
        syncButton.disabled = false;
    }
});
