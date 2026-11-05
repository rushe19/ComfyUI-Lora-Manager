// lmx can be served at the site root (the phone PWA, a direct LAN visit) or below a prefix such as /lmx/
// (inside Forge Neo, through its proxy). Absolute paths like '/api/...' are resolved against that prefix.
// The page URL never changes while the app runs (overlays use history entries with no URL), so the
// document base is the directory the app was loaded from.

/** Directory the app is served from, always with a trailing slash: '/' or '/lmx/'. */
export function baseFrom(docBase: string): string {
  const p = new URL('.', docBase).pathname
  return p.endsWith('/') ? p : p + '/'
}

export const BASE = typeof document === 'undefined' ? '/' : baseFrom(document.baseURI)

/** Prefix a root-absolute path ('/api/x') with the base; anything else is returned unchanged. */
export function withBase(path: string, base: string = BASE): string {
  if (base === '/' || !path.startsWith('/') || path.startsWith('//')) return path
  if (path.startsWith(base)) return path
  return base + path.slice(1)
}

/** True when served from the site root (the installable PWA case). */
export const atRoot = BASE === '/'
