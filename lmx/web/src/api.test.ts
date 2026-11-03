import { afterEach, describe, expect, it, vi } from 'vitest'
import { fetchHealth } from './api'

const respond = (body: string, contentType: string, status = 200) =>
  vi.fn().mockResolvedValue(new Response(body, { status, headers: { 'content-type': contentType } }))

afterEach(() => vi.unstubAllGlobals())

describe('fetchHealth', () => {
  it('parses a JSON health response', async () => {
    vi.stubGlobal('fetch', respond('{"status":"ok","name":"lmx","version":"0.0.1"}', 'application/json'))
    await expect(fetchHealth()).resolves.toEqual({ status: 'ok', name: 'lmx', version: '0.0.1' })
  })

  it('rejects an SPA HTML fallback instead of treating it as healthy', async () => {
    vi.stubGlobal('fetch', respond('<!doctype html>', 'text/html'))
    await expect(fetchHealth()).rejects.toThrow(/content-type/)
  })

  it('rejects non-2xx', async () => {
    vi.stubGlobal('fetch', respond('{}', 'application/json', 503))
    await expect(fetchHealth()).rejects.toThrow('HTTP 503')
  })
})
