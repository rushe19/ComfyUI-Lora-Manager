// Embedded mode: lmx shown inside Forge Neo (a panel in the Forge page, served under /lmx/ by the
// sd-forge-lmx extension). The page talks to its parent with postMessage; everything is a no-op when
// lmx is opened on its own.
import { reactive } from 'vue'
import type { ModelDetail } from './api'
import { toast } from './toast'

export const isEmbedded = (() => {
  try {
    return new URLSearchParams(location.search).get('embed') === 'forge' && window.parent !== window
  } catch {
    return false
  }
})()

/** What Forge last told us: the tab prompts are added to and the active checkpoint. */
export const forge = reactive<{ tab: string; checkpoint: string | null }>({ tab: 'txt2img', checkpoint: null })

export type ToForge =
  | { type: 'lmx:hello' }
  | { type: 'lmx:close' }
  | { type: 'lmx:add-network'; kind: 'lora'; name: string; weight: number | null; trigger_words: string[] }
  | { type: 'lmx:add-network'; kind: 'embedding'; name: string; negative: boolean }
  | { type: 'lmx:set-checkpoint'; file_path: string; name: string }

/** File name without folders and extension: what Forge calls the model inside <lora:...>. */
export const fileStem = (path: string): string => (path.split(/[\\/]/).pop() ?? '').replace(/\.[^.]*$/, '')

/** The message that applies this model in Forge, or null for kinds Forge has no use for. */
export function buildApplyMessage(d: ModelDetail, opt: { negative?: boolean } = {}): ToForge | null {
  switch (d.summary.type) {
    case 'lora':
      return { type: 'lmx:add-network', kind: 'lora', name: fileStem(d.file_path), weight: d.strength > 0 ? d.strength : null, trigger_words: d.trained_words }
    case 'embedding':
      return { type: 'lmx:add-network', kind: 'embedding', name: fileStem(d.file_path), negative: opt.negative === true }
    case 'checkpoint':
      return { type: 'lmx:set-checkpoint', file_path: d.file_path, name: d.file_path.split(/[\\/]/).pop() ?? '' }
    default:
      return null
  }
}

export function sendToForge(msg: ToForge) {
  if (isEmbedded) window.parent.postMessage(msg, location.origin)
}

/** Apply a model in Forge (no-op for kinds Forge cannot use). */
export function applyInForge(d: ModelDetail, opt: { negative?: boolean } = {}) {
  const m = buildApplyMessage(d, opt)
  if (m) sendToForge(m)
}

/** React to a message from Forge: remember its context, tell the user what happened. */
export function handleForgeMessage(data: unknown) {
  if (!data || typeof data !== 'object') return
  const m = data as Record<string, unknown>
  if (m.type === 'forge:context') {
    if (typeof m.tab === 'string') forge.tab = m.tab
    forge.checkpoint = typeof m.checkpoint === 'string' ? m.checkpoint : null
  } else if (m.type === 'forge:applied') {
    const name = String(m.name ?? '')
    if (m.kind === 'checkpoint') toast(`Checkpoint を切り替えました: ${name}`, 2200)
    else if (m.action === 'added') toast(`${m.negative ? 'ネガティブに' : ''}追加しました: ${name}`, 1800)
    else if (m.action === 'removed') toast(`プロンプトから外しました: ${name}`, 1800)
    else toast('変更はありませんでした')
  } else if (m.type === 'forge:error') {
    toast(String(m.message ?? 'Forge でエラーが起きました'), 4000)
  }
}

export function initEmbed() {
  if (!isEmbedded) return
  document.documentElement.classList.add('embedded')
  window.addEventListener('message', (ev) => {
    if (ev.origin !== location.origin || ev.source !== window.parent) return
    handleForgeMessage(ev.data)
  })
  sendToForge({ type: 'lmx:hello' })
}
