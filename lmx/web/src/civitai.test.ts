import { describe, expect, it } from 'vitest'
import { buildCivitaiQuery } from './api'
import { normalizeFolder, suggestFolders } from './targets'

describe('buildCivitaiQuery', () => {
  it('defaults to nsfw on and omits empty values', () => {
    expect(buildCivitaiQuery({ type: 'lora', q: ' ', cursor: null })).toBe('type=lora&nsfw=1')
  })

  it('encodes all filters', () => {
    const q = Object.fromEntries(
      new URLSearchParams(
        buildCivitaiQuery({ type: 'checkpoint', q: 'frieren', base: ['Illustrious', 'Pony'], sort: 'Most Downloaded', period: 'Month', nsfw: false, cursor: 40, favorites: true }),
      ),
    )
    expect(q).toEqual({ type: 'checkpoint', q: 'frieren', base: 'Illustrious,Pony', sort: 'Most Downloaded', period: 'Month', nsfw: '0', cursor: '40', favorites: '1' })
  })
})

describe('normalizeFolder', () => {
  it('cleans separators and empty parts', () => {
    expect(normalizeFolder('  /Character\\\\Frieren/ ')).toBe('Character/Frieren')
    expect(normalizeFolder('./a//b/')).toBe('a/b')
    expect(normalizeFolder('')).toBe('')
  })
})

describe('suggestFolders', () => {
  const folders = ['Anima', 'Anima/guro', 'Character', 'Krea2', 'MOSS', 'Qwen', 'Qwen/edit']
  it('matches base model names loosely', () => {
    expect(suggestFolders(folders, 'Krea 2')).toEqual(['Krea2'])
    expect(suggestFolders(folders, 'Qwen')).toEqual(['Qwen'])
    expect(suggestFolders(folders, 'Anima')).toEqual(['Anima'])
  })
  it('returns nothing for short or unrelated names', () => {
    expect(suggestFolders(folders, 'Illustrious')).toEqual([])
    expect(suggestFolders(folders, 'XL')).toEqual([])
  })
})
