// Service worker for the document app, registered at the site root so the
// engine's module worker under lib/ is in scope too. The build prepends the
// files of one build, each with a hash of its contents, and a version derived
// from them. A page this worker controls is served entirely from that build,
// the shell included, so it never mixes files from two deployments; a newer
// build installs beside it and takes over when the app says so (main.js). The
// API and rooms are never intercepted.
const VERSION = self.__XMD_VERSION__ || "dev";
const PRECACHE = self.__XMD_PRECACHE__ || [];
const CACHE = `xmd-docs-${VERSION}`;
const site = new URL(self.registration.scope); // the site root: docs/ and lib/ live beneath it
const shell = new URL("docs/", site);
const manifest = new URL("xmd-precache.json", site); // what each cache holds, never fetched
// A navigation cannot be answered with a redirected response, and some hosts
// redirect index.html to the directory, so the shell is stored as a plain copy.
const plain = async response => new Response(await response.arrayBuffer(), { status: 200, headers: { "content-type": response.headers.get("content-type") || "text/html; charset=utf-8" } });
const precached = new Set(PRECACHE.map(e => new URL(e.url, site).pathname));
const hex = async response => [...new Uint8Array(await crypto.subtle.digest("SHA-256", await response.arrayBuffer()))].map(b => b.toString(16).padStart(2, "0")).join("");

self.addEventListener("install", event => {
  event.waitUntil((async () => {
    // Files unchanged since an earlier build are copied from its cache, not downloaded again.
    const earlier = new Map();
    for (const key of await caches.keys()) {
      if (!key.startsWith("xmd-docs-") || key === CACHE) continue;
      const cache = await caches.open(key);
      const held = await cache.match(manifest);
      for (const e of held ? await held.json().catch(() => []) : []) earlier.set(`${e.url} ${e.revision}`, cache);
    }
    const cache = await caches.open(CACHE);
    await Promise.all(PRECACHE.map(async ({ url, revision }) => {
      const href = new URL(url, site).href;
      const kept = await earlier.get(`${url} ${revision}`)?.match(href);
      if (kept) return cache.put(href, kept);
      // A deployment landing mid-install would hand over files from another
      // build; checking each against its hash fails the install instead, and
      // the browser tries again with the newer worker.
      const response = await fetch(new Request(href, { cache: "reload" }));
      if (!response.ok || !(await hex(response.clone())).startsWith(revision)) throw new Error(`${url} does not match this build`);
      await cache.put(href, response);
    }));
    await cache.put(shell.href, await plain(await fetch(new Request(shell.href, { cache: "reload" }))));
    await cache.put(manifest, new Response(JSON.stringify(PRECACHE), { headers: { "content-type": "application/json" } }));
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
  if (request.mode === "navigate") {
    // The app shell of this worker's build; the network only until it is cached.
    if (url.pathname.startsWith(shell.pathname)) event.respondWith((async () => (await caches.match(shell.href, { cacheName: CACHE })) || fetch(request))());
    return;
  }
  // The build's files, cache first. Anything else (shared links, the sync API) is not part of a build.
  if (!precached.has(url.pathname)) return;
  const cached = () => caches.match(request, { cacheName: CACHE, ignoreVary: true });
  event.respondWith((async () => {
    // Only the app is pinned to this worker's build. A page outside it, like
    // the book, loads lib/ beside files of its own that no build holds, so it
    // takes the network's, as does the engine worker it starts (`?fresh`);
    // this build's copy is only for when the network is gone.
    if (!(await forApp(event.clientId))) return fetch(request).catch(async () => (await cached()) || Response.error());
    return (await cached()) || fetch(request);
  })());
});

/** Whether a request comes from the app: one of its pages, or a worker none
 * of the other pages marked `?fresh`. */
async function forApp(id) {
  const client = id && await self.clients.get(id);
  if (!client) return true;
  const at = new URL(client.url);
  return client.type === "window" ? at.pathname.startsWith(shell.pathname) : !at.searchParams.has("fresh");
}
