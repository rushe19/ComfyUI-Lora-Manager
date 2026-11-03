// UI preferences persisted in localStorage.
import { reactive, watch } from 'vue'

export type Theme = 'system' | 'dark' | 'light'

export interface Prefs {
  theme: Theme
  blurMature: boolean
  showVideos: boolean
}

const KEY = 'lmx.prefs.v1'

function load(): Prefs {
  const defaults: Prefs = { theme: 'system', blurMature: false, showVideos: true }
  try {
    return { ...defaults, ...(JSON.parse(localStorage.getItem(KEY) ?? '{}') as Partial<Prefs>) }
  } catch {
    return defaults
  }
}

export const prefs = reactive<Prefs>(load())

export function applyTheme(theme: Theme) {
  const dark = theme === 'dark' || (theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches)
  document.documentElement.dataset.theme = dark ? 'dark' : 'light'
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', dark ? '#14122a' : '#f6f5fb')
}

watch(
  prefs,
  (p) => {
    localStorage.setItem(KEY, JSON.stringify(p))
    applyTheme(p.theme)
  },
  { deep: true },
)
window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => applyTheme(prefs.theme))
