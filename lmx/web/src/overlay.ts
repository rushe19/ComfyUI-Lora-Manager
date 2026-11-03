// Tiny history-backed overlay stack: the Android back button closes the top sheet instead of leaving the app.
import { ref } from 'vue'

export const overlays = ref<string[]>([])

export function openOverlay(name: string) {
  overlays.value = [...overlays.value, name]
  history.pushState({ lmxOverlay: name }, '')
}

/** Close via history so the browser stack stays in sync. */
export function closeOverlay() {
  if (overlays.value.length) history.back()
}

window.addEventListener('popstate', () => {
  if (overlays.value.length) overlays.value = overlays.value.slice(0, -1)
})

export const isOpen = (name: string) => overlays.value.includes(name)
