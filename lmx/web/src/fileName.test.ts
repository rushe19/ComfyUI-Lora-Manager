import { describe, expect, it } from 'vitest'
import { autoStem, chosenName, splitFileName } from './fileName'

describe('splitFileName', () => {
  it('splits at the last dot', () => {
    expect(splitFileName('a.v2.safetensors')).toEqual({ stem: 'a.v2', ext: '.safetensors' })
    expect(splitFileName('plain')).toEqual({ stem: 'plain', ext: '' })
    expect(splitFileName('.hidden')).toEqual({ stem: '.hidden', ext: '' })
  })
})

describe('chosenName', () => {
  it('is undefined while the name is unchanged or empty', () => {
    expect(chosenName('frieren', 'frieren.safetensors')).toBeUndefined()
    expect(chosenName('  ', 'frieren.safetensors')).toBeUndefined()
  })
  it('puts the original extension back', () => {
    expect(chosenName(' my frieren ', 'frieren.safetensors')).toBe('my frieren.safetensors')
  })
})

describe('autoStem', () => {
  it('joins model name and version name', () => {
    expect(autoStem('WAI-illustrious-SDXL', 'v16.0')).toBe('WAI-illustrious-SDXL_v16.0')
    expect(autoStem('Frieren', '')).toBe('Frieren')
  })
  it('replaces characters a file name cannot contain', () => {
    expect(autoStem('Foo: the "Bar"/Baz?', 'v1|2')).toBe('Foo_ the _Bar_Baz_v1_2')
  })
  it('keeps the version when the model name is too long', () => {
    const s = autoStem('a'.repeat(300), 'v2.0')
    expect(s.length).toBeLessThanOrEqual(150)
    expect(s.endsWith('_v2.0')).toBe(true)
  })
  it('uses underscores instead of spaces for embeddings only', () => {
    expect(autoStem('My Embed', 'v 1', 'embedding')).toBe('My_Embed_v_1')
    expect(autoStem('My Lora', 'v 1', 'lora')).toBe('My Lora_v 1')
  })
  it('keeps Japanese names', () => {
    expect(autoStem('葬送のフリーレン', 'v1.0')).toBe('葬送のフリーレン_v1.0')
  })
})
