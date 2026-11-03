// Transient toast message.
import { ref } from 'vue'

export const toastText = ref('')
let timer: number | undefined

export function toast(text: string, ms = 1600) {
  toastText.value = text
  window.clearTimeout(timer)
  timer = window.setTimeout(() => (toastText.value = ''), ms)
}
