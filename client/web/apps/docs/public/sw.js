// Service worker for the document app, registered at the site root so the
// engine's module worker under lib/ is in scope too. The build appends a
// precache list and a version; the app never depends on it, it only gets
// faster and works without a network. The API and rooms are never intercepted.
const VERSION = self.__XMD_VERSION__ || "dev";
const PRECACHE = self.__XMD_PRECACHE__ || [];
const CACHE = `xmd-docs-${VERSION}`;
const site = new URL(self.registration.scope); // the site root: docs/ and lib/ live beneath it
const shell = new URL("docs/", site);
// A navigation cannot be answered with a redirected response, and some hosts
// redirect index.html to the directory, so the shell is stored as a plain copy.
const plain = async response => new Response(await response.arrayBuffer(), { status: 200, headers: { "content-type": response.headers.get("content-type") || "text/html; charset=utf-8" } });
const precached = new Set(PRECACHE.map(p => new URL(p, site).pathname));

self.addEventListener("install", event => {
  event.waitUntil((async () => {
    const cache = await caches.open(CACHE);
    await cache.addAll(PRECACHE.map(p => new Request(new URL(p, site).href, { cache: "reload" })));
    await cache.put(shell.href, await plain(await fetch(new Request(shell.href, { cache: "reload" }))));
  })());
});
self.addEventListener("activate", event => {
  event.waitUntil((async () => {
    for (const key of await caches.keys()) if (key.startsWith("xmd-docs-") && key !== CACHE) await caches.delete(key);
    await self.clients.claim();
  })());
});
self.addEventListener("message", event => { if (event.data === "skip-waiting") self.skipWaiting(); });

self.addEventListener("fetch", event => {
  const { request } = event;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (url.origin !== location.origin) return;
  if (url.pathname.includes("/api/")) return; // accounts, rooms, and sign-in always go to the network
  const path = url.pathname;
  const inSite = path.startsWith(site.pathname);
  if (!inSite) return;
  if (request.mode === "navigate") {
    // The app shell: network when available, the cached page otherwise.
    event.respondWith((async () => {
      try {
        const fresh = await fetch(request);
        if (fresh.ok && url.pathname.startsWith(shell.pathname)) plain(fresh.clone()).then(async copy => (await caches.open(CACHE)).put(shell.href, copy)).catch(() => {});
        return fresh;
      } catch { return (await caches.match(shell.href)) || Response.error(); }
    })());
    return;
  }
  if (precached.has(path)) {
    // Hashed assets, the library, the engine: cache first.
    event.respondWith((async () => (await caches.match(request)) || fetch(request).then(async r => { if (r.ok) (await caches.open(CACHE)).put(request, r.clone()); return r; }))());
    return;
  }
  // Anything else under the site (a deployment's backend module, its chunks):
  // network first, remembered for offline.
  event.respondWith((async () => {
    try { const r = await fetch(request); if (r.ok) (await caches.open(CACHE)).put(request, r.clone()); return r; }
    catch { return (await caches.match(request)) || Response.error(); }
  })());
});
