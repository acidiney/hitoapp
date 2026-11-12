const CACHE = "hito-static-v1";
const OFFLINE_KEY = "/__hito-offline";
const NAVIGATION_TIMEOUT_MS = 5000;
const PRECACHED_ICONS = ["/icon-192.png", "/favicon.svg"];
const HASHED_ASSET =
  /^\/assets\/[^/]+-[A-Za-z0-9_-]{8}\.(?:js|css|woff2|svg|png)$/;

const OFFLINE_PAGE = `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover"><title>Hito</title><style>
:root{color-scheme:light dark;--bg:#fbfbfa;--ink:#141414;--muted:#5f5e5a;--action:#2f4bff;--on-action:#fff;--ring:#6f86ff}
@media (prefers-color-scheme:dark){:root{--bg:#111110;--ink:#f2f1ed;--muted:#a3a29c;--action:#6f86ff;--on-action:#111110}}
*{box-sizing:border-box}
body{margin:0;min-height:100dvh;display:grid;place-items:center;padding:24px;background:var(--bg);color:var(--ink);font-family:Figtree,system-ui,-apple-system,"Segoe UI",sans-serif}
main{max-width:420px;text-align:center}
svg{display:block;margin:0 auto 24px}
h1{margin:0 0 12px;font-size:28px;font-weight:800;letter-spacing:-.01em}
p{margin:0 0 24px;font-size:16px;line-height:1.5;color:var(--muted)}
button{min-height:44px;padding:0 24px;border:0;border-radius:10px;background:var(--action);color:var(--on-action);font:inherit;font-weight:700;cursor:pointer}
button:focus-visible{outline:2px solid var(--ring);outline-offset:2px}
</style></head><body><main><svg width="72" height="20" viewBox="0 0 72 20" aria-hidden="true"><circle cx="10" cy="10" r="8" fill="currentColor"/><circle cx="36" cy="10" r="7" fill="none" stroke="var(--ring)" stroke-width="2.4"/><circle cx="62" cy="10" r="7" fill="none" stroke="var(--muted)" stroke-width="2.4" stroke-dasharray="1 4.4" stroke-linecap="round"/></svg><h1>Hito can't reach your server</h1><p>Check that your server is running and that this device is online. Your place is kept.</p><button type="button" id="retry">Retry</button></main><script>document.getElementById("retry").addEventListener("click",function(){location.reload()});window.addEventListener("online",function(){location.reload()})</script></body></html>`;

function offlineResponse() {
  return new Response(OFFLINE_PAGE, {
    status: 503,
    headers: {
      "Content-Type": "text/html; charset=utf-8",
      "Cache-Control": "no-store",
    },
  });
}

function isCacheableAsset(request, url) {
  return (
    request.method === "GET" &&
    url.origin === self.location.origin &&
    url.search === "" &&
    !request.headers.has("range") &&
    !request.headers.has("authorization") &&
    HASHED_ASSET.test(url.pathname)
  );
}

function isNavigation(request, url) {
  return (
    request.mode === "navigate" &&
    request.method === "GET" &&
    url.origin === self.location.origin &&
    !url.pathname.startsWith("/api/")
  );
}

function withTimeout(promise, ms) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("timeout")), ms);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error) => {
        clearTimeout(timer);
        reject(error);
      },
    );
  });
}

async function navigate(request) {
  try {
    return await withTimeout(fetch(request), NAVIGATION_TIMEOUT_MS);
  } catch {
    const cache = await caches.open(CACHE);
    return (await cache.match(OFFLINE_KEY)) || offlineResponse();
  }
}

async function cacheFirst(request) {
  const cache = await caches.open(CACHE);
  const hit = await cache.match(request);
  if (hit) return hit;
  const response = await fetch(request);
  if (response.status === 200 && response.type === "basic") {
    await cache.put(request, response.clone());
  }
  return response;
}

async function install() {
  const cache = await caches.open(CACHE);
  await cache.put(OFFLINE_KEY, offlineResponse());
  await Promise.all(
    PRECACHED_ICONS.map((path) => cache.add(path).catch(() => undefined)),
  );
}

async function activate() {
  const names = await caches.keys();
  await Promise.all(
    names.filter((name) => name !== CACHE).map((name) => caches.delete(name)),
  );
  await self.clients.claim();
}

self.addEventListener("install", (event) => {
  event.waitUntil(install());
});

self.addEventListener("activate", (event) => {
  event.waitUntil(activate());
});

self.addEventListener("message", (event) => {
  if (event.data && event.data.type === "SKIP_WAITING") self.skipWaiting();
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  const url = new URL(request.url);
  if (isNavigation(request, url)) {
    event.respondWith(navigate(request));
  } else if (isCacheableAsset(request, url)) {
    event.respondWith(cacheFirst(request));
  }
});
