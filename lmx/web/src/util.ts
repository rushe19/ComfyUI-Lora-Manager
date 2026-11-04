// Pure helpers (unit-tested): virtual grid layout, formatting, text extraction.

export interface GridLayout {
  cols: number
  cardWidth: number
  rowHeight: number
}

/** Fit as many columns of at least `minCard` px as possible into `width` (min 2 on phones). */
export function computeGrid(width: number, minCard: number, gap: number, captionHeight: number): GridLayout {
  const usable = Math.max(0, width - gap)
  const cols = Math.max(2, Math.floor(usable / (minCard + gap)))
  const cardWidth = Math.floor(usable / cols - gap)
  const rowHeight = Math.round((cardWidth * 4) / 3 + captionHeight + gap)
  return { cols, cardWidth, rowHeight }
}

/** Rows to render for a scroll position, with overscan, clamped to [0, rows). */
export function visibleRows(scrollTop: number, viewport: number, rowHeight: number, rows: number, overscan = 3): [number, number] {
  if (rows <= 0 || rowHeight <= 0) return [0, 0]
  const first = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan)
  const last = Math.min(rows, Math.ceil((scrollTop + viewport) / rowHeight) + overscan)
  return [first, Math.max(first, last)]
}

export function formatSize(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GB`
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`
  return `${bytes} B`
}

export function formatDate(unixSecs: number): string {
  if (!unixSecs) return ''
  const d = new Date(unixSecs * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

/** Civitai descriptions are HTML from third parties: never inject them, show their text only. */
export function htmlToText(html: string): string {
  if (!html) return ''
  const withBreaks = html.replace(/<\s*br\s*\/?>/gi, '\n').replace(/<\/(p|div|li|h\d)>/gi, '\n')
  const doc = new DOMParser().parseFromString(withBreaks, 'text/html')
  return (doc.body.textContent ?? '').replace(/\n{3,}/g, '\n\n').trim()
}

/** Copy text; falls back to execCommand on plain-http LAN origins where the Clipboard API is unavailable. */
export async function copyText(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text)
      return true
    }
  } catch {
    /* fall through */
  }
  const ta = document.createElement('textarea')
  ta.value = text
  ta.setAttribute('readonly', '')
  ta.style.position = 'fixed'
  ta.style.opacity = '0'
  document.body.appendChild(ta)
  ta.select()
  let ok = false
  try {
    ok = document.execCommand('copy')
  } finally {
    ta.remove()
  }
  return ok
}

/** Civitai NSFW levels: 1 PG, 2 PG-13, 4 R, 8 X, 16 XXX. Blur from R upwards. */
export const isMature = (level: number) => level >= 4

/** Civitai ISO timestamp -> local `YYYY/MM/DD` (with time when asked). */
export function formatIsoDate(iso: string | null | undefined, withTime = false): string {
  if (!iso) return ''
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return ''
  const p = (n: number) => String(n).padStart(2, '0')
  const date = `${d.getFullYear()}/${p(d.getMonth() + 1)}/${p(d.getDate())}`
  return withTime ? `${date} ${p(d.getHours())}:${p(d.getMinutes())}` : date
}

/** Time left until `iso`: "あと3日" / "あと5時間" / "あと20分" ('' when past). */
export function timeLeft(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) return ''
  const ms = new Date(iso).getTime() - now
  if (!(ms > 0)) return ''
  const h = ms / 3_600_000
  if (h >= 24) return `あと${Math.floor(h / 24)}日`
  if (h >= 1) return `あと${Math.floor(h)}時間`
  return `あと${Math.max(1, Math.floor(ms / 60_000))}分`
}

/** Short badge text for a download restriction. */
export function accessBadge(a: { kind: 'early' | 'paid'; ends_at: string | null } | null | undefined, now = Date.now()): string {
  if (!a) return ''
  if (a.kind === 'paid') return '要Buzz'
  const left = timeLeft(a.ends_at, now)
  return left ? `先行公開 ${left}` : '先行公開'
}

/** True for Civitai/other media URLs that point at a video. */
export const isVideoUrl = (url: string) => /\.(mp4|webm)(\?|$)/i.test(url) || /\/transcode=/.test(url)

/** Small (450 px) rendition of a Civitai CDN media url for thumbnails; other urls are returned unchanged. */
export function thumbUrl(url: string, isVideo: boolean): string {
  const re = /\/(original=true|width=\d+|transcode=[^/]*)[^/]*\//
  if (!re.test(url)) return url
  return url.replace(re, isVideo ? '/transcode=true,width=450,optimized=true/' : '/width=450/')
}

/** A sharper-than-thumbnail rendition for the fullscreen viewer (Civitai CDN stills only). */
export function viewerUrl(url: string, isVideo: boolean, width = 1280): string {
  if (isVideo) return url
  return url.replace(/\/(original=true|width=\d+)[^/]*\//, `/width=${width}/`)
}
