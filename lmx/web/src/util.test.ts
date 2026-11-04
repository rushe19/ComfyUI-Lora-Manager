import { describe, expect, it } from 'vitest'
import { computeGrid, formatSize, htmlToText, isVideoUrl, thumbUrl, viewerUrl, visibleRows } from './util'
import { buildListQuery } from './api'

describe('htmlToText', () => {
  it('keeps text and line breaks but never markup', () => {
    const t = htmlToText('<p>Hello <b>world</b></p><p>line2<br>line3</p><img src=x onerror="alert(1)"><script>bad()</script>')
    expect(t).toContain('Hello world')
    expect(t).toContain('line2\nline3')
    expect(t).not.toContain('<')
    expect(t).not.toContain('onerror')
  })
})

describe('computeGrid', () => {
  it('uses at least two columns on a narrow phone', () => {
    const g = computeGrid(360, 160, 8, 48)
    expect(g.cols).toBe(2)
    expect(g.cardWidth).toBeGreaterThan(160)
    expect(g.rowHeight).toBe(Math.round((g.cardWidth * 4) / 3 + 48 + 8))
  })

  it('fits more columns on desktop', () => {
    expect(computeGrid(1600, 180, 12, 48).cols).toBe(8)
  })
})

describe('visibleRows', () => {
  it('returns the window plus overscan, clamped', () => {
    expect(visibleRows(0, 800, 200, 100, 2)).toEqual([0, 6])
    expect(visibleRows(2000, 800, 200, 100, 2)).toEqual([8, 16])
    expect(visibleRows(19900, 800, 200, 100, 2)).toEqual([97, 100])
  })

  it('handles empty grids', () => {
    expect(visibleRows(0, 800, 200, 0)).toEqual([0, 0])
  })
})

describe('buildListQuery', () => {
  it('omits defaults', () => {
    expect(buildListQuery({ type: 'lora', q: '  ', sort: 'name', offset: 0 })).toBe('type=lora')
  })

  it('encodes filters', () => {
    const q = new URLSearchParams(
      buildListQuery({ type: 'lora', q: 'hatsune miku', base: ['Illustrious', 'Pony'], folder: 'Char', recursive: false, fav: true, damaged: true, sort: 'date', desc: true, offset: 120, limit: 60 }),
    )
    expect(Object.fromEntries(q)).toEqual({
      type: 'lora', q: 'hatsune miku', base: 'Illustrious,Pony', folder: 'Char', recursive: '0', fav: '1', damaged: '1', sort: 'date', order: 'desc', offset: '120', limit: '60',
    })
  })

  it('keeps an explicit root folder filter', () => {
    expect(buildListQuery({ type: 'lora', folder: '' })).toBe('type=lora&folder=')
  })
})

describe('formatSize', () => {
  it('formats', () => {
    expect(formatSize(512)).toBe('512 B')
    expect(formatSize(167.5 * 1024 * 1024)).toBe('167.5 MB')
    expect(formatSize(6.5 * 1024 ** 3)).toBe('6.50 GB')
  })
})

describe('media urls', () => {
  it('detects video urls', () => {
    expect(isVideoUrl('https://image.civitai.com/k/u/transcode=true,width=450,optimized=true/a.mp4')).toBe(true)
    expect(isVideoUrl('https://x/y/clip.WEBM?x=1')).toBe(true)
    expect(isVideoUrl('https://image.civitai.com/k/u/width=450/a.jpeg')).toBe(false)
  })
  it('asks for a bigger still for the viewer but leaves videos alone', () => {
    expect(viewerUrl('https://image.civitai.com/k/u/original=true/a.jpeg', false)).toBe('https://image.civitai.com/k/u/width=1280/a.jpeg')
    expect(viewerUrl('https://image.civitai.com/k/u/width=450/a.jpeg', false)).toBe('https://image.civitai.com/k/u/width=1280/a.jpeg')
    expect(viewerUrl('https://example.com/a.png', false)).toBe('https://example.com/a.png')
    expect(viewerUrl('https://image.civitai.com/k/u/original=true/a.mp4', true)).toBe('https://image.civitai.com/k/u/original=true/a.mp4')
  })
})

describe('thumbUrl', () => {
  it('uses the small rendition for civitai media only', () => {
    expect(thumbUrl('https://image.civitai.com/k/u/original=true/a.mp4', true)).toBe('https://image.civitai.com/k/u/transcode=true,width=450,optimized=true/a.mp4')
    expect(thumbUrl('https://image.civitai.com/k/u/original=true/a.jpeg', false)).toBe('https://image.civitai.com/k/u/width=450/a.jpeg')
    expect(thumbUrl('https://other.example/a.mp4', true)).toBe('https://other.example/a.mp4')
  })
})
