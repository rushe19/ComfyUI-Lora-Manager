import { afterEach, describe, expect, it, vi } from 'vitest'
import { auth, watchAuth } from './auth'

describe('watchAuth', () => {
  afterEach(() => vi.restoreAllMocks())
  it('flags login required only for marked 401s', async () => {
    const responses = [
      new Response('{}', { status: 401, headers: { 'x-lmx-auth': 'required' } }),
      new Response('{}', { status: 401 }),
    ]
    window.fetch = vi.fn().mockImplementation(() => Promise.resolve(responses.shift()))
    watchAuth()
    auth.required = false
    await fetch('/api/other')
    expect(auth.required).toBe(true)
    auth.required = false
    await fetch('/api/login-failure')
    expect(auth.required).toBe(false)
  })
})
