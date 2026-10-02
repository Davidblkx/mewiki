"use strict";

const VERSION = "__VERSION__";
const STATIC = `static-${VERSION}`;
const PAGES = "pages";
const OFFLINE_PAGE = "/_/static/offline.html";

self.addEventListener("install", (event) => {
    event.waitUntil(
        caches
            .open(STATIC)
            .then((cache) => cache.addAll(["/_/static/style.css", OFFLINE_PAGE]))
            .then(() => self.skipWaiting()),
    );
});

self.addEventListener("activate", (event) => {
    event.waitUntil(
        caches
            .keys()
            .then((names) =>
                Promise.all(
                    names.filter((name) => name.startsWith("static-") && name !== STATIC).map((name) => caches.delete(name)),
                ),
            )
            .then(() => self.clients.claim()),
    );
});

async function cacheFirst(request) {
    const cache = await caches.open(STATIC);
    const cached = await cache.match(request);
    if (cached) return cached;
    const response = await fetch(request);
    if (response.ok) await cache.put(request, response.clone());
    return response;
}

async function networkFirst(request) {
    const cache = await caches.open(PAGES);
    let response;
    try {
        response = await fetch(request);
    } catch {
        const cached = await cache.match(request);
        if (cached) return cached;
        if (request.mode === "navigate") return (await caches.match(OFFLINE_PAGE)) ?? Response.error();
        return Response.error();
    }
    if (response.status === 404) {
        await cache.delete(request);
    } else if (response.ok) {
        await cache.put(request, response.clone());
    }
    return response;
}

self.addEventListener("fetch", (event) => {
    const request = event.request;
    const url = new URL(request.url);
    if (request.method !== "GET" || url.origin !== self.location.origin) return;
    if (url.pathname.startsWith("/_/static/")) {
        event.respondWith(cacheFirst(request));
    } else if (url.pathname === "/_/custom.css") {
        event.respondWith(networkFirst(request));
    } else if (url.pathname.startsWith("/_/") || url.pathname === "/sw.js" || url.pathname === "/manifest.webmanifest") {
        return;
    } else {
        event.respondWith(networkFirst(request));
    }
});
