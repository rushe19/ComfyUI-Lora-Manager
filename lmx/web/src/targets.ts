// Remembered download destination per model type.
import type { ModelType } from './api'

export interface Target {
  root: string | null
  folder: string
}

const KEY = 'lmx.targets.v1'

function all(): Record<string, Target> {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? '{}') as Record<string, Target>
  } catch {
    return {}
  }
}

export function lastTarget(type: ModelType): Target {
  return all()[type] ?? { root: null, folder: '' }
}

export function rememberTarget(type: ModelType, t: Target) {
  const a = all()
  a[type] = t
  localStorage.setItem(KEY, JSON.stringify(a))
}

/** Normalize a user-typed folder: forward slashes, no leading/trailing slashes, no empty parts. */
export function normalizeFolder(input: string): string {
  return input
    .replace(/\\/g, '/')
    .split('/')
    .map((p) => p.trim())
    .filter((p) => p && p !== '.')
    .join('/')
}

const squash = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, '')

/** Top-level folders whose name loosely matches the base model ("Krea 2" ~ "Krea2"); names under 3 chars never match. */
export function suggestFolders(folders: string[], baseModel: string): string[] {
  const b = squash(baseModel)
  if (b.length < 3) return []
  const tops = new Set<string>()
  for (const f of folders) {
    const top = f.split('/')[0]
    const t = squash(top)
    if (t.length >= 3 && (b.startsWith(t) || t.startsWith(b))) tops.add(top)
  }
  return [...tops].slice(0, 4)
}
