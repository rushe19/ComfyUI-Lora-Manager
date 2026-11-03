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
  linked: boolean
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
  exclude: boolean
  skip_metadata_refresh: boolean
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
  unlinked?: boolean
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
  if (p.unlinked) s.set('unlinked', '1')
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

// ---- Civitai & downloads ----

export interface CivImage {
  url: string
  full: string
  type: string
  nsfw_level: number
  width: number | null
  height: number | null
}

export interface CivCard {
  id: number
  name: string
  type: string
  lmx_type: ModelType
  nsfw: boolean
  nsfw_level: number
  creator: string | null
  tags: string[]
  downloads: number
  thumbs_up: number
  base_models: string[]
  versions: number
  owned_versions: number
  queued: boolean
  image: CivImage | null
  url: string
}

export interface CivFile {
  id: number
  name: string
  size_kb: number
  type: string
  fp: string | null
  format: string | null
  primary: boolean
  sha256: string | null
  owned_local_id: number | null
}

export interface CivVersion {
  id: number
  name: string
  base_model: string
  published_at: string | null
  availability: string | null
  trained_words: string[]
  downloads: number
  owned_local_id: number | null
  job: string | null
  description: string
  images: CivImage[]
  files: CivFile[]
}

export interface CivModel {
  id: number
  name: string
  type: string
  lmx_type: ModelType
  nsfw: boolean
  creator: string | null
  tags: string[]
  description: string
  url: string
  versions: CivVersion[]
}

export interface CivSearchParams {
  q?: string
  type: ModelType | 'all'
  base?: string[]
  sort?: string
  period?: string
  nsfw?: boolean
  cursor?: string | number | null
  favorites?: boolean
}

export interface CivSearchPage {
  items: CivCard[]
  next_cursor: string | number | null
  has_key: boolean
  /** Set when lmx sorted a keyword search itself (Civitai ignores sort with a keyword). */
  resorted: { pool: number; capped: boolean } | null
}

export interface BaseModelPrefs {
  all: string[]
  selected: string[]
  new: string[]
  local: { name: string; count: number }[]
  fetched_at: number | null
  error: string | null
}

export type JobStatus = 'queued' | 'running' | 'done' | 'failed' | 'canceled'

export interface DownloadJob {
  id: number
  created_at: number
  updated_at: number
  status: JobStatus
  model_type: ModelType
  model_id: number
  version_id: number
  file_id: number | null
  model_name: string
  version_name: string
  base_model: string
  file_name: string
  dest_dir: string
  size_bytes: number
  sha256: string | null
  bytes_done: number
  error: string | null
  image_url: string | null
  nsfw_level: number
  result_path: string | null
}

export interface Targets {
  type: ModelType
  roots: string[]
  folders: string[]
}

export class ApiError extends Error {
  constructor(
    public code: string,
    message: string,
    public status: number,
  ) {
    super(message)
  }
}

async function send<T>(method: string, path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
  const res = await fetch(path, {
    method,
    headers: body === undefined ? { Accept: 'application/json' } : { Accept: 'application/json', 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal,
  })
  const ct = res.headers.get('content-type') ?? ''
  const data = ct.includes('application/json') ? await res.json() : null
  if (!res.ok) {
    const d = (data ?? {}) as { error?: string; message?: string }
    throw new ApiError(d.error ?? 'http', d.message ?? `HTTP ${res.status}`, res.status)
  }
  if (data === null) throw new Error(`unexpected content-type: ${ct}`)
  return data as T
}

export function buildCivitaiQuery(p: CivSearchParams): string {
  const s = new URLSearchParams()
  s.set('type', p.type)
  if (p.q && p.q.trim()) s.set('q', p.q.trim())
  if (p.base && p.base.length) s.set('base', p.base.join(','))
  if (p.sort) s.set('sort', p.sort)
  if (p.period) s.set('period', p.period)
  s.set('nsfw', p.nsfw === false ? '0' : '1')
  if (p.cursor != null && p.cursor !== '') s.set('cursor', String(p.cursor))
  if (p.favorites) s.set('favorites', '1')
  return s.toString()
}

export const civitaiSearch = (p: CivSearchParams, signal?: AbortSignal) => send<CivSearchPage>('GET', `/api/civitai/search?${buildCivitaiQuery(p)}`, undefined, signal)
export const civitaiModel = (id: number) => send<CivModel>('GET', `/api/civitai/models/${id}`)
export const downloadTargets = (type: ModelType) => send<Targets>('GET', `/api/download-targets?type=${type}`)
export const enqueueDownload = (b: { model_id: number; version_id: number; file_id?: number | null; root?: string | null; folder: string }) =>
  send<{ job: DownloadJob }>('POST', '/api/downloads', b)
export const enqueueUrls = (b: { text: string; type: ModelType; root?: string | null; folder: string }) =>
  send<{ results: { input: string; ok: boolean; job?: DownloadJob; error?: string }[] }>('POST', '/api/downloads/urls', b)
export const listDownloads = () => send<{ jobs: DownloadJob[] }>('GET', '/api/downloads')
export const cancelDownload = (id: number) => send<{ result: string }>('DELETE', `/api/downloads/${id}`)
export const retryDownload = (id: number) => send<{ result: string }>('POST', `/api/downloads/${id}/retry`)

// ---- Library writes (Phase 3) ----

export interface EditBody {
  model_name?: string
  tags?: string[]
  notes?: string
  favorite?: boolean
  exclude?: boolean
  base_model?: string
  trained_words?: string[]
  strength?: number
  repair?: boolean
}

export interface TrashEntry {
  id: number
  at: number
  model_name: string
  model_type: ModelType
  paths: string[]
  restored: boolean
}

export type MetadataMode = 'archive_first' | 'archive_only' | 'civitai_first'

export interface MetaJob {
  id: number
  mode: MetadataMode | null
  running: boolean
  canceled: boolean
  started_at: number
  finished_at: number | null
  total: number
  done: number
  current: string
  archive_hits: number
  civitai_hits: number
  unresolved: number
  skipped: number
  hashed: number
  errors: number
  api_requests: number
  image_requests: number
  previews_added: number
  backup_dir: string | null
  recent_errors: string[]
  elapsed_secs: number
}

export interface MetaTarget {
  type?: ModelType
  ids?: number[]
  folder?: string
  mode?: MetadataMode
  only_unlinked?: boolean
  previews?: boolean
  retry_misses?: boolean
}

export interface MetaPreview {
  total: number
  in_archive: number
  no_hash: number
  archive: { path: string; hashes: number } | null
  default_mode: MetadataMode
}

export interface DupGroup {
  sha256: string
  size: number
  items: { summary: ModelSummary; file_path: string }[]
}

export const editModel = (id: number, b: EditBody) => send<ModelDetail>('PATCH', `/api/models/${id}`, b)
export const moveModel = (id: number, b: { root?: string | null; folder?: string; name?: string }) =>
  send<{ path: string; id: number | null }>('POST', `/api/models/${id}/move`, b)
export const deleteModel = (id: number) => send<{ trash: TrashEntry }>('DELETE', `/api/models/${id}`)
export const listTrash = () => send<{ items: TrashEntry[] }>('GET', '/api/trash')
export const restoreTrash = (id: number) => send<{ restored: TrashEntry }>('POST', `/api/trash/${id}/restore`)
export const bulkAction = (b: { ids: number[]; action: string; value?: unknown; root?: string | null; folder?: string }) =>
  send<{ ok: number; errors: string[]; trash_ids: number[] }>('POST', '/api/bulk', b)
export const fetchDuplicates = (type: ModelType) => send<{ groups: DupGroup[]; wasted_bytes: number }>('GET', `/api/duplicates?type=${type}`)
export const metadataPreview = (t: MetaTarget) => send<MetaPreview>('POST', '/api/metadata/preview', t)
export const startMetadata = (t: MetaTarget) => send<{ job: MetaJob }>('POST', '/api/metadata/fetch', t)
export const metadataJob = () => send<{ job: MetaJob | null }>('GET', '/api/metadata/job')
export const cancelMetadata = () => send<{ canceling: boolean }>('DELETE', '/api/metadata/job')

export const fetchBaseModelPrefs = () => send<BaseModelPrefs>('GET', '/api/civitai/base-models')
export const saveBaseModelPrefs = (selected: string[]) => send<BaseModelPrefs>('PUT', '/api/civitai/base-models', { selected })
