import { describe, expect, it } from 'vitest'
import { splitList } from './actions'

describe('splitList', () => {
  it('splits on commas, Japanese commas and newlines and trims', () => {
    expect(splitList(' a, b、c\n\n d ,')).toEqual(['a', 'b', 'c', 'd'])
    expect(splitList('')).toEqual([])
  })
})
