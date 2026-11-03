// Transient toast message, optionally with one action button (e.g. 元に戻す).
import { ref } from 'vue'

export const toastText = ref('')
export const toastAction = ref<{ label: string; run: () => void } | null>(null)
let timer: number | undefined

export function toast(text: string, ms = 1600, action?: { label: string; run: () => void }) {
  toastText.value = text
  toastAction.value = action ?? null
  window.clearTimeout(timer)
  timer = window.setTimeout(
    () => {
      toastText.value = ''
      toastAction.value = null
    },
    action ? Math.max(ms, 6000) : ms,
  )
}

export function runToastAction() {
  const a = toastAction.value
  window.clearTimeout(timer)
  toastText.value = ''
  toastAction.value = null
  a?.run()
}
