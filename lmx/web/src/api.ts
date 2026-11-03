// Typed client for the lmx REST API.

export type ModelType = 'lora' | 'checkpoint' | 'embedding' | 'other'

export interface Health {
  status: string
  name: string
  version: string
}

export interface ModelSummary {
  id: number
  type: ModelType
  name: string
  file_name: string
  folder: string
  base_model: string
  size: number
  modified: number
  favorite: boolean
  nsfw_level: number
  has_metadata: boolean
  damaged: boolean
  preview: string | null
  preview_kind: 'image' | 'video' | null
}

export interface ModelPage {
  total: number
  offset: number
  next_offset: number | null
  items: ModelSummary[]
}

export interface CivitaiImage {
  url: string
  nsfw_level: number | null
  type: string
  width: number | null
  height: number | null
}

export interface ModelDetail {
  summary: ModelSummary
  file_path: string
  root: string
  sha256: string | null
  tags: string[]
  trained_words: string[]
  creator: string | null
  civitai_model_id: number | null
  civitai_version_id: number | null
  civitai_url: string | null
  version_name: string
  metadata_source: string | null
  damage: string | null
  sidecar_error: string | null
  model_description: string
  version_description: string
  notes: string
  usage_tips: string | null
  strength: number
  lora_syntax: string | null
  images: CivitaiImage[]
}

export interface Count {
  name: string
  count: number
}

export interface FolderCount {
  path: string
  direct: number
  total: number
}

export interface Facets {
  base_models: Count[]
  tags: Count[]
}

export interface ScanStats {
  models: number
  unchanged: number
  parsed: number
  removed: number
  damaged: number
  sidecar_errors: number
  missing_sidecars: number
  walk_secs: number
  parse_secs: number
  total_secs: number
}

export interface Status {
  total: number
  counts: Partial<Record<ModelType, number>>
  scanning: boolean
  progress: { walked_files: number; to_parse: number; parsed: number }
  last_scan: { finished_at: number; stats: ScanStats; error: string | null } | null
  roots: Record<ModelType, string[]>
  civitai_host: string
}

export type SortKey = 'name' | 'date' | 'size'

export interface ListParams {
  type: ModelType
  q?: string
  base?: string[]
  tag?: string
  folder?: string | null
  recursive?: boolean
  fav?: boolean
  damaged?: boolean
  sort?: SortKey
  desc?: boolean
  offset?: number
  limit?: number
}

/** Build the query string for /api/models (omits defaults so URLs stay short). */
export function buildListQuery(p: ListParams): string {
  const s = new URLSearchParams()
  s.set('type', p.type)
  if (p.q && p.q.trim()) s.set('q', p.q.trim())
  if (p.base && p.base.length) s.set('base', p.base.join(','))
  if (p.tag) s.set('tag', p.tag)
  if (p.folder != null) {
    s.set('folder', p.folder)
    if (p.recursive === false) s.set('recursive', '0')
  }
  if (p.fav) s.set('fav', '1')
  if (p.damaged) s.set('damaged', '1')
  if (p.sort && p.sort !== 'name') s.set('sort', p.sort)
  if (p.desc) s.set('order', 'desc')
  if (p.offset) s.set('offset', String(p.offset))
  if (p.limit) s.set('limit', String(p.limit))
  return s.toString()
}

async function getJson<T>(path: string, signal?: AbortSignal): Promise<T> {
  const res = await fetch(path, { headers: { Accept: 'application/json' }, signal })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  const ct = res.headers.get('content-type') ?? ''
  // An SPA fallback answers unknown paths with HTML; treat that as "server too old", not success.
  if (!ct.includes('application/json')) throw new Error(`unexpected content-type: ${ct}`)
  return (await res.json()) as T
}

export const fetchHealth = (base = '') => getJson<Health>(`${base}/api/health`)
export const fetchStatus = () => getJson<Status>('/api/status')
export const fetchModels = (p: ListParams, signal?: AbortSignal) => getJson<ModelPage>(`/api/models?${buildListQuery(p)}`, signal)
export const fetchDetail = (id: number) => getJson<ModelDetail>(`/api/models/${id}`)
export const fetchFolders = (type: ModelType) => getJson<FolderCount[]>(`/api/folders?type=${type}`)
export const fetchFacets = (type: ModelType) => getJson<Facets>(`/api/facets?type=${type}`)

export async function startScan(): Promise<boolean> {
  const res = await fetch('/api/scan', { method: 'POST' })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return ((await res.json()) as { started: boolean }).started
}
