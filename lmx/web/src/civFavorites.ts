// Which Civitai models are favorites of the API key's user. Filled lazily for the ids on screen,
// updated when the heart is tapped, so the grid and the detail sheet agree.
import { reactive } from 'vue'
import { civitaiFavorites, setCivitaiFavorite } from './api'

export const favs = reactive({ ids: new Set<number>(), checked: new Set<number>(), available: true })

/** Look up the ids we have not asked about yet (the server splits into 200 per Civitai request). */
export async function checkFavorites(ids: number[]): Promise<void> {
  const todo = ids.filter((i) => !favs.checked.has(i))
  if (!todo.length || !favs.available) return
  try {
    const r = await civitaiFavorites(todo)
    if (!r.has_key) {
      favs.available = false
      return
    }
    for (const i of todo) favs.checked.add(i)
    for (const i of r.favorite) favs.ids.add(i)
  } catch (e) {
    // No key: stop asking. Any other failure is retried with the next page.
    if ((e as { code?: string }).code === 'needs_key') favs.available = false
  }
}

/** Mark a list that is known to be all favorites (the Civitai favorites tab) without asking. */
export function markAllFavorite(ids: number[]) {
  for (const i of ids) {
    favs.ids.add(i)
    favs.checked.add(i)
  }
}

export async function toggleFavorite(id: number, on: boolean): Promise<boolean> {
  const r = await setCivitaiFavorite(id, on)
  favs.checked.add(id)
  if (r.favorite) favs.ids.add(id)
  else favs.ids.delete(id)
  return r.favorite
}
