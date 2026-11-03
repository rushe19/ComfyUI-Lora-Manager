<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'
import { fetchFacets, fetchFolders, fetchModels, fetchStatus, type Facets, type FolderCount, type ModelSummary, type ModelType, type SortKey } from '../api'
import { isOpen, openOverlay, closeOverlay } from '../overlay'
import ModelGrid from './ModelGrid.vue'
import DetailSheet from './DetailSheet.vue'
import BulkBar from './BulkBar.vue'

const props = defineProps<{ active?: boolean }>()

const TYPES: { id: ModelType; label: string }[] = [
  { id: 'lora', label: 'LoRA' },
  { id: 'checkpoint', label: 'Checkpoint' },
  { id: 'embedding', label: 'Embedding' },
  { id: 'other', label: 'その他' },
]
const PAGE = 120

const f = reactive({
  type: (localStorage.getItem('lmx.type') as ModelType) || 'lora',
  q: '',
  base: [] as string[],
  folder: null as string | null,
  fav: false,
  damaged: false,
  unlinked: false,
  sort: (localStorage.getItem('lmx.sort') as SortKey) || 'name',
  desc: localStorage.getItem('lmx.desc') === '1',
})
const counts = ref<Partial<Record<ModelType, number>>>({})
const items = ref<ModelSummary[]>([])
const total = ref(0)
const nextOffset = ref<number | null>(0)
const loading = ref(false)
const error = ref('')
const facets = ref<Facets>({ base_models: [], tags: [] })
const folders = ref<FolderCount[]>([])
const folderFilter = ref('')
const selectedId = ref<number | null>(null)
let generation = 0
let ctrl: AbortController | null = null

const activeFilters = computed(() => f.base.length + (f.folder != null ? 1 : 0) + (f.fav ? 1 : 0) + (f.damaged ? 1 : 0) + (f.unlinked ? 1 : 0))

// Selection mode (long press or the 選択 button).
const selecting = ref(false)
const selected = ref<Set<number>>(new Set())
function toggleSelect(id: number) {
  const s = new Set(selected.value)
  if (s.has(id)) s.delete(id)
  else s.add(id)
  selected.value = s
}
function startSelect(id?: number) {
  selecting.value = true
  if (id != null) toggleSelect(id)
}
function endSelect() {
  selecting.value = false
  selected.value = new Set()
}
function onCard(id: number) {
  if (selecting.value) toggleSelect(id)
  else openDetail(id)
}
function selectAllLoaded() {
  selected.value = new Set(items.value.map((i) => i.id))
}
function afterBulk(clear: boolean) {
  if (clear) endSelect()
  void softRefresh()
}
function refreshAll() {
  void softRefresh()
}
/** Reload what is already loaded without jumping back to the top. */
async function softRefresh() {
  const gen = ++generation
  ctrl?.abort()
  ctrl = new AbortController()
  try {
    const page = await fetchModels(
      { type: f.type, q: f.q, base: f.base, folder: f.folder, fav: f.fav, damaged: f.damaged, unlinked: f.unlinked, sort: f.sort, desc: f.desc, offset: 0, limit: Math.max(items.value.length, PAGE) },
      ctrl.signal,
    )
    if (gen !== generation) return
    items.value = page.items
    total.value = page.total
    nextOffset.value = page.next_offset
    loading.value = false
  } catch (e) {
    if (gen === generation && (e as Error).name !== 'AbortError') error.value = String(e)
  }
  void loadFacets()
  void refreshCounts()
}
function reopen(id: number) {
  selectedId.value = id
}
const shownFolders = computed(() => {
  const needle = folderFilter.value.trim().toLowerCase()
  const list = folders.value.filter((x) => x.path !== '')
  return (needle ? list.filter((x) => x.path.toLowerCase().includes(needle)) : list).slice(0, 300)
})

async function loadMore() {
  if (loading.value || nextOffset.value == null) return
  loading.value = true
  const gen = generation
  ctrl = new AbortController()
  try {
    const page = await fetchModels(
      { type: f.type, q: f.q, base: f.base, folder: f.folder, fav: f.fav, damaged: f.damaged, unlinked: f.unlinked, sort: f.sort, desc: f.desc, offset: nextOffset.value, limit: PAGE },
      ctrl.signal,
    )
    if (gen !== generation) return
    items.value = items.value.concat(page.items)
    total.value = page.total
    nextOffset.value = page.next_offset
    error.value = ''
  } catch (e) {
    if (gen === generation && (e as Error).name !== 'AbortError') error.value = String(e)
  } finally {
    if (gen === generation) loading.value = false
  }
}

function reset() {
  generation++
  ctrl?.abort()
  loading.value = false
  items.value = []
  nextOffset.value = 0
  void loadMore()
}

async function loadFacets() {
  const t = f.type
  const [fa, fo] = await Promise.all([fetchFacets(t), fetchFolders(t)])
  if (t === f.type) {
    facets.value = fa
    folders.value = fo
  }
}

let debounce: number | undefined
watch(
  () => f.q,
  () => {
    window.clearTimeout(debounce)
    debounce = window.setTimeout(reset, 250)
  },
)
watch(
  () => [f.base.join(','), f.folder, f.fav, f.damaged, f.unlinked, f.sort, f.desc],
  () => {
    localStorage.setItem('lmx.sort', f.sort)
    localStorage.setItem('lmx.desc', f.desc ? '1' : '0')
    reset()
  },
)
watch(
  () => f.type,
  (t) => {
    localStorage.setItem('lmx.type', t)
    f.base = []
    f.folder = null
    endSelect()
    reset()
    void loadFacets()
  },
)

function toggleBase(name: string) {
  f.base = f.base.includes(name) ? f.base.filter((b) => b !== name) : [...f.base, name]
}
function clearFilters() {
  f.base = []
  f.folder = null
  f.fav = false
  f.damaged = false
  f.unlinked = false
}
function openDetail(id: number) {
  selectedId.value = id
  openOverlay('detail')
}
function openFilters() {
  openOverlay('filters')
}

async function refreshCounts() {
  try {
    const st = await fetchStatus()
    const changed = lastTotal !== null && st.total !== lastTotal
    lastTotal = st.total
    counts.value = st.counts
    return changed
  } catch {
    return false
  }
}
let lastTotal: number | null = null

// Coming back to the library (e.g. after a download): reload if the library changed.
watch(
  () => props.active,
  async (a) => {
    if (a && (await refreshCounts())) {
      void softRefresh()
    }
  },
)

onMounted(() => {
  reset()
  void loadFacets()
  void refreshCounts()
})
</script>

<template>
  <section class="library">
    <header class="lib-header">
      <div class="type-tabs" role="tablist">
        <button v-for="t in TYPES" :key="t.id" role="tab" :class="{ active: f.type === t.id }" @click="f.type = t.id">
          {{ t.label }}<small v-if="counts[t.id] != null">{{ counts[t.id] }}</small>
        </button>
      </div>
      <div class="search-row">
        <input v-model="f.q" type="search" placeholder="名前・トリガーワード・タグで検索" enterkeyhint="search" aria-label="検索" />
        <button class="filter-btn" :class="{ on: activeFilters > 0 }" @click="openFilters">
          絞り込み<span v-if="activeFilters">{{ activeFilters }}</span>
        </button>
        <button class="filter-btn" :class="{ on: selecting }" @click="selecting ? endSelect() : startSelect()">{{ selecting ? '完了' : '選択' }}</button>
      </div>
      <div class="result-line">
        <span>{{ total.toLocaleString() }} 件</span>
        <span v-if="f.folder != null" class="chip" @click="f.folder = null">📁 {{ f.folder || '(ルート)' }} ✕</span>
        <span v-for="b in f.base" :key="b" class="chip" @click="toggleBase(b)">{{ b }} ✕</span>
        <span v-if="f.fav" class="chip" @click="f.fav = false">★ お気に入り ✕</span>
        <span v-if="f.damaged" class="chip warn" @click="f.damaged = false">要修復 ✕</span>
        <span v-if="f.unlinked" class="chip" @click="f.unlinked = false">メタデータ未取得 ✕</span>
      </div>
    </header>

    <p v-if="error" class="err">読み込みに失敗しました: {{ error }}</p>
    <ModelGrid :items="items" :total="total" :loading="loading" :selected="selecting ? selected : undefined" @need-more="loadMore" @open="onCard" @long="startSelect" />
    <BulkBar v-if="selecting" :ids="[...selected]" :type="f.type" @done="afterBulk" @select-all="selectAllLoaded" @clear="selected = new Set()" />

    <div v-if="isOpen('filters')" class="sheet-backdrop" @click.self="closeOverlay">
      <div class="sheet filters">
        <header class="sheet-head">
          <strong>絞り込み・並べ替え</strong>
          <button class="link" @click="clearFilters">クリア</button>
          <button class="close" aria-label="閉じる" @click="closeOverlay">✕</button>
        </header>
        <div class="sheet-body">
          <h4>並べ替え</h4>
          <div class="seg">
            <button v-for="s in (['name', 'date', 'size'] as SortKey[])" :key="s" :class="{ active: f.sort === s }" @click="f.sort = s">
              {{ s === 'name' ? '名前' : s === 'date' ? '更新日' : 'サイズ' }}
            </button>
            <button :class="{ active: f.desc }" @click="f.desc = !f.desc">{{ f.desc ? '降順 ↓' : '昇順 ↑' }}</button>
          </div>
          <h4>表示</h4>
          <label class="check"><input v-model="f.fav" type="checkbox" /> お気に入りのみ</label>
          <label class="check"><input v-model="f.damaged" type="checkbox" /> 要修復（メタデータ破損）のみ</label>
          <label class="check"><input v-model="f.unlinked" type="checkbox" /> メタデータ未取得（Civitai 情報なし）のみ</label>
          <h4>ベースモデル</h4>
          <div class="chips">
            <button v-for="b in facets.base_models" :key="b.name" class="chip" :class="{ active: f.base.includes(b.name) }" @click="toggleBase(b.name)">
              {{ b.name }} <small>{{ b.count }}</small>
            </button>
          </div>
          <h4>フォルダ</h4>
          <input v-model="folderFilter" type="search" placeholder="フォルダ名で絞り込み" />
          <ul class="folder-list">
            <li :class="{ active: f.folder === null }" @click="f.folder = null">すべて</li>
            <li v-for="d in shownFolders" :key="d.path" :class="{ active: f.folder === d.path }" :style="{ paddingLeft: 8 + (d.path.split('/').length - 1) * 14 + 'px' }" @click="f.folder = d.path">
              {{ d.path.split('/').pop() }} <small>{{ d.total }}</small>
            </li>
          </ul>
        </div>
      </div>
    </div>

    <DetailSheet v-if="isOpen('detail') && selectedId != null" :id="selectedId" @close="closeOverlay" @changed="refreshAll" @reopen="reopen" />
  </section>
</template>
