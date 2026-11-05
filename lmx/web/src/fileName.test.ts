import { describe, expect, it } from 'vitest'
import { chosenName, splitFileName } from './fileName'

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
