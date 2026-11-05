import { beforeEach, describe, expect, it } from 'vitest'
import type { ModelDetail } from './api'
import { buildApplyMessage, fileStem, forge, handleForgeMessage } from './embed'
import { toastText } from './toast'

const detail = (type: string, extra: Partial<ModelDetail> = {}) =>
  ({ summary: { type }, file_path: 'Z:\\models\\Lora\\Anime\\my.lora.v2.safetensors', trained_words: ['a girl', 'red hair'], strength: 0.8, ...extra }) as unknown as ModelDetail

beforeEach(() => {
  toastText.value = ''
  forge.tab = 'txt2img'
  forge.checkpoint = null
})

describe('embed', () => {
  it('uses the file name without folders and the last extension', () => {
    expect(fileStem('Z:\\m\\Lora\\a\\my.lora.v2.safetensors')).toBe('my.lora.v2')
    expect(fileStem('/m/x/名前_01.safetensors')).toBe('名前_01')
    expect(fileStem('plain')).toBe('plain')
  })

  it('builds the message for each kind', () => {
    expect(buildApplyMessage(detail('lora'))).toEqual({ type: 'lmx:add-network', kind: 'lora', name: 'my.lora.v2', weight: 0.8, trigger_words: ['a girl', 'red hair'] })
    expect(buildApplyMessage(detail('lora', { strength: 0 }))).toMatchObject({ weight: null })
    expect(buildApplyMessage(detail('embedding'), { negative: true })).toEqual({ type: 'lmx:add-network', kind: 'embedding', name: 'my.lora.v2', negative: true })
    expect(buildApplyMessage(detail('checkpoint'))).toEqual({ type: 'lmx:set-checkpoint', file_path: 'Z:\\models\\Lora\\Anime\\my.lora.v2.safetensors', name: 'my.lora.v2.safetensors' })
    expect(buildApplyMessage(detail('other'))).toBeNull()
  })

  it('remembers the context Forge reports', () => {
    handleForgeMessage({ type: 'forge:context', tab: 'img2img', checkpoint: 'moss.safetensors' })
    expect(forge).toMatchObject({ tab: 'img2img', checkpoint: 'moss.safetensors' })
    handleForgeMessage({ type: 'forge:context', tab: 'txt2img', checkpoint: 5 })
    expect(forge.checkpoint).toBeNull()
  })

  it('tells the user what happened', () => {
    handleForgeMessage({ type: 'forge:applied', kind: 'lora', name: 'foo', action: 'added' })
    expect(toastText.value).toBe('追加しました: foo')
    handleForgeMessage({ type: 'forge:applied', kind: 'embedding', name: 'bad', action: 'added', negative: true })
    expect(toastText.value).toBe('ネガティブに追加しました: bad')
    handleForgeMessage({ type: 'forge:applied', kind: 'lora', name: 'foo', action: 'removed' })
    expect(toastText.value).toBe('プロンプトから外しました: foo')
    handleForgeMessage({ type: 'forge:applied', kind: 'checkpoint', name: 'moss', action: 'set' })
    expect(toastText.value).toContain('Checkpoint を切り替えました')
    handleForgeMessage({ type: 'forge:error', message: '見つかりません' })
    expect(toastText.value).toBe('見つかりません')
  })

  it('ignores junk', () => {
    for (const j of [null, 'x', 5, {}, { type: 'other' }]) handleForgeMessage(j)
    expect(toastText.value).toBe('')
  })
})
