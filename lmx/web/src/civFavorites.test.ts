import { beforeEach, describe, expect, it, vi } from 'vitest'
import { checkFavorites, favs, markAllFavorite, toggleFavorite } from './civFavorites'

const json = (body: unknown, status = 200) => Promise.resolve(new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json' } }))

beforeEach(() => {
  favs.ids = new Set()
  favs.checked = new Set()
  favs.available = true
  vi.restoreAllMocks()
})

describe('civFavorites', () => {
  it('asks only about ids it has not checked yet', async () => {
    const f = vi.fn().mockImplementation(() => json({ favorite: [2], has_key: true }))
    window.fetch = f as never
    await checkFavorites([1, 2, 3])
    expect([...favs.ids]).toEqual([2])
    await checkFavorites([2, 3, 4])
    expect(f).toHaveBeenCalledTimes(2)
    expect(String(f.mock.calls[1][0])).toContain('ids=4')
    await checkFavorites([1, 4])
    expect(f).toHaveBeenCalledTimes(2)
  })

  it('stops asking when there is no api key', async () => {
    const f = vi.fn().mockImplementation(() => json({ error: 'needs_key', message: 'key' }, 400))
    window.fetch = f as never
    await checkFavorites([1])
    expect(favs.available).toBe(false)
    await checkFavorites([2])
    expect(f).toHaveBeenCalledTimes(1)
  })

  it('toggle follows the state Civitai reports', async () => {
    window.fetch = vi.fn().mockImplementation(() => json({ favorite: true })) as never
    expect(await toggleFavorite(9, true)).toBe(true)
    expect(favs.ids.has(9)).toBe(true)
    window.fetch = vi.fn().mockImplementation(() => json({ favorite: false })) as never
    expect(await toggleFavorite(9, false)).toBe(false)
    expect(favs.ids.has(9)).toBe(false)
  })

  it('favorites tab marks everything without asking', () => {
    const f = vi.fn()
    window.fetch = f as never
    markAllFavorite([5, 6])
    expect(favs.ids.has(5) && favs.ids.has(6)).toBe(true)
    return checkFavorites([5, 6]).then(() => expect(f).not.toHaveBeenCalled())
  })
})
