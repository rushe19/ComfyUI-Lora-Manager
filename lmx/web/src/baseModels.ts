// Base models offered in Civitai search, shared by the search screen and the settings screen.
// One reactive store: changing the selection in settings updates the search chips at once.
import { reactive } from 'vue'
import { fetchBaseModelPrefs, saveBaseModelPrefs, type BaseModelPrefs } from './api'

export const baseModels = reactive({
  loaded: false,
  saving: false,
  error: '',
  all: [] as string[],
  selected: [] as string[],
  fresh: [] as string[],
  local: {} as Record<string, number>,
})

function apply(p: BaseModelPrefs) {
  baseModels.all = p.all
  baseModels.selected = p.selected
  baseModels.fresh = p.new
  baseModels.local = Object.fromEntries(p.local.map((l) => [l.name, l.count]))
  baseModels.error = p.error ?? ''
  baseModels.loaded = true
}

let pending: Promise<void> | null = null
export function loadBaseModels(force = false): Promise<void> {
  if (!force && (baseModels.loaded || pending)) return pending ?? Promise.resolve()
  pending = fetchBaseModelPrefs()
    .then(apply)
    .catch((e) => {
      baseModels.error = String(e)
    })
    .finally(() => {
      pending = null
    })
  return pending
}

/** Keep the order of the official list so chips don't jump around. */
export function orderLike(all: string[], names: string[]): string[] {
  const set = new Set(names)
  const known = all.filter((n) => set.has(n))
  return known.concat(names.filter((n) => !all.includes(n)))
}

let saveTimer: number | undefined
export function setSelected(names: string[]) {
  baseModels.selected = orderLike(baseModels.all, names)
  window.clearTimeout(saveTimer)
  saveTimer = window.setTimeout(async () => {
    baseModels.saving = true
    try {
      apply(await saveBaseModelPrefs(baseModels.selected))
    } catch (e) {
      baseModels.error = String(e)
    } finally {
      baseModels.saving = false
    }
  }, 400)
}

export function toggleSelected(name: string) {
  const s = baseModels.selected
  setSelected(s.includes(name) ? s.filter((x) => x !== name) : [...s, name])
}
