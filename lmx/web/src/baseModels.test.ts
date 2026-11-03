import { describe, expect, it } from 'vitest'
import { orderLike } from './baseModels'

describe('orderLike', () => {
  it('follows the official order and keeps unknown names at the end', () => {
    expect(orderLike(['Anima', 'Illustrious', 'MOSS Image', 'Pony'], ['Pony', 'Custom', 'Anima'])).toEqual(['Anima', 'Pony', 'Custom'])
  })
})
