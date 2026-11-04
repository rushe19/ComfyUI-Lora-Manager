// lmx service worker (Phase 0): cache the app shell, never cache the API.
// Bump SHELL_VERSION when the shell list changes; hashed /assets/* are cached on first use.
const SHELL_VERSION = 'lmx-shell-v2'
const SHELL = ['/', '/manifest.webmanifest', '/icons/icon-192.png', '/icons/icon-512.png']

self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(SHELL_VERSION).then((c) => c.addAll(SHELL)).then(() => self.skipWaiting()))
})

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== SHELL_VERSION).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  )
})

self.addEventListener('fetch', (event) => {
  const req = event.request
  if (req.method !== 'GET') return
  const url = new URL(req.url)
  if (url.origin !== self.location.origin) return
  if (url.pathname.startsWith('/api/')) return // network only

  // Navigations: network first so a new build is picked up, fall back to the cached shell offline.
  if (req.mode === 'navigate') {
    event.respondWith(
      // cache: 'no-cache' makes the browser revalidate instead of reusing a heuristically fresh index.html.
      fetch(req.url, { cache: 'no-cache', credentials: 'same-origin' })
        .then((res) => {
          const copy = res.clone()
          caches.open(SHELL_VERSION).then((c) => c.put('/', copy))
          return res
        })
        .catch(() => caches.match('/')),
    )
    return
  }

  // Static assets: cache first (Vite asset names are content-hashed).
  event.respondWith(
    caches.match(req).then(
      (hit) =>
        hit ||
        fetch(req).then((res) => {
          if (res.ok && (url.pathname.startsWith('/assets/') || url.pathname.startsWith('/icons/'))) {
            const copy = res.clone()
            caches.open(SHELL_VERSION).then((c) => c.put(req, copy))
          }
          return res
        }),
    ),
  )
})
