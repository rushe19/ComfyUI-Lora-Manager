import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createApp, h, nextTick } from 'vue'
import LazyVideo from './components/LazyVideo.vue'
import { overlays } from './overlay'

type Cb = (e: Array<{ isIntersecting: boolean }>) => void
let cb: Cb = () => {}
const play = vi.fn(() => Promise.resolve())

function mountVideo(props: { src: string; pauseBehindOverlay?: boolean }) {
  const host = document.createElement('div')
  document.body.appendChild(host)
  createApp({ render: () => h(LazyVideo, props) }).mount(host)
  return host.querySelector('video') as HTMLVideoElement
}

beforeEach(() => {
  overlays.value = []
  play.mockClear()
  vi.stubGlobal('IntersectionObserver', class { constructor(c: Cb) { cb = c } observe() {} disconnect() {} })
  HTMLMediaElement.prototype.play = play as never
  HTMLMediaElement.prototype.pause = vi.fn()
  HTMLMediaElement.prototype.load = vi.fn()
})
afterEach(() => {
  vi.unstubAllGlobals()
  document.body.innerHTML = ''
})

describe('LazyVideo', () => {
  it('loads only while on screen and releases the source when it leaves', () => {
    const v = mountVideo({ src: '/a.mp4' })
    expect(v.getAttribute('src')).toBeNull()
    cb([{ isIntersecting: true }])
    expect(v.getAttribute('src')).toBe('/a.mp4')
    expect(play).toHaveBeenCalled()
    cb([{ isIntersecting: false }])
    expect(v.getAttribute('src')).toBeNull()
  })
  it('lets go while a sheet covers the list', async () => {
    const v = mountVideo({ src: '/a.mp4', pauseBehindOverlay: true })
    cb([{ isIntersecting: true }])
    expect(v.getAttribute('src')).toBe('/a.mp4')
    overlays.value = ['detail']
    await nextTick()
    expect(v.getAttribute('src')).toBeNull()
    overlays.value = []
    await nextTick()
    expect(v.getAttribute('src')).toBe('/a.mp4')
  })
})
