import { describe, expect, it } from 'vitest'
import { baseFrom, withBase } from './base'

describe('base path', () => {
  it('finds the directory the app was loaded from', () => {
    expect(baseFrom('http://h:9100/')).toBe('/')
    expect(baseFrom('http://h:9100/index.html')).toBe('/')
    expect(baseFrom('http://h:7860/lmx/')).toBe('/lmx/')
    expect(baseFrom('http://h:7860/lmx/?embed=forge')).toBe('/lmx/')
    expect(baseFrom('http://h:7860/sub/lmx/')).toBe('/sub/lmx/')
  })
  it('prefixes root-absolute paths only', () => {
    expect(withBase('/api/models?type=lora', '/lmx/')).toBe('/lmx/api/models?type=lora')
    expect(withBase('/api/preview/3?v=1', '/')).toBe('/api/preview/3?v=1')
    expect(withBase('https://cdn.example/x.mp4', '/lmx/')).toBe('https://cdn.example/x.mp4')
    expect(withBase('//cdn.example/x.mp4', '/lmx/')).toBe('//cdn.example/x.mp4')
    expect(withBase('relative/x', '/lmx/')).toBe('relative/x')
    expect(withBase('blob:abc', '/lmx/')).toBe('blob:abc')
  })
  it('does not prefix twice', () => {
    expect(withBase('/lmx/api/x', '/lmx/')).toBe('/lmx/api/x')
  })
})
