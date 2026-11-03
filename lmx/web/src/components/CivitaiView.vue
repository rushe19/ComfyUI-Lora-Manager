<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { ApiError, civitaiSearch, enqueueUrls, fetchFacets, type CivCard, type ModelType } from '../api'
import { closeOverlay, isOpen, openOverlay } from '../overlay'
import { prefs } from '../prefs'
import { normalizeFolder, rememberTarget, type Target } from '../targets'
import { toast } from '../toast'
import { isMature } from '../util'
import CivitaiModelSheet from './CivitaiModelSheet.vue'
import TargetPicker from './TargetPicker.vue'

type Mode = 'search' | 'favorites' | 'url'
const mode = ref<Mode>('search')
const TYPES: { id: ModelType; label: string }[] = [
  { id: 'lora', label: 'LoRA' },
  { id: 'checkpoint', label: 'Checkpoint' },
  { id: 'embedding', label: 'Embedding' },
  { id: 'other', label: 'その他' },
]
const SORTS = ['Highest Rated', 'Most Downloaded', 'Newest']
const SORT_LABEL: Record<string, string> = { 'Highest Rated': '評価順', 'Most Downloaded': 'DL 数順', Newest: '新着' }
const PERIODS = ['AllTime', 'Year', 'Month', 'Week', 'Day']
const PERIOD_LABEL: Record<string, string> = { AllTime: '全期間', Year: '1 年', Month: '1 か月', Week: '1 週間', Day: '1 日' }

const f = reactive({
  q: '',
  type: 'lora' as ModelType,
  base: [] as string[],
  sort: 'Highest Rated',
  period: 'AllTime',
  nsfw: true,
})
const items = ref<CivCard[]>([])
const cursor = ref<string | number | null>(null)
const loading = ref(false)
const error = ref('')
const hasKey = ref(true)
const searched = ref(false)
const baseChoices = ref<string[]>([])
const selected = ref<number | null>(null)
const sentinel = ref<HTMLElement | null>(null)
let gen = 0
let ctrl: AbortController | null = null

async function run(reset: boolean) {
  if (loading.value && !reset) return
  if (reset) {
    gen++
    ctrl?.abort()
    items.value = []
    cursor.value = null
  } else if (cursor.value == null) {
    return
  }
  const my = gen
  loading.value = true
  error.value = ''
  ctrl = new AbortController()
  try {
    const page = await civitaiSearch(
      { q: mode.value === 'search' ? f.q : '', type: f.type, base: f.base, sort: f.sort, period: f.period, nsfw: f.nsfw, cursor: cursor.value, favorites: mode.value === 'favorites' },
      ctrl.signal,
    )
    if (my !== gen) return
    items.value = items.value.concat(page.items)
    cursor.value = page.next_cursor
    hasKey.value = page.has_key
    searched.value = true
  } catch (e) {
    if (my !== gen || (e as Error).name === 'AbortError') return
    error.value = e instanceof ApiError && e.code === 'needs_key' ? 'お気に入りの表示には Civitai の API キーが必要です（lmx.toml か LoRA Manager の設定）。' : (e as Error).message
  } finally {
    if (my === gen) loading.value = false
  }
}

function submit() {
  void run(true)
}

watch(
  () => [f.type, f.base.join(','), f.sort, f.period, f.nsfw, mode.value],
  () => {
    if (mode.value !== 'url' && (searched.value || mode.value === 'favorites')) void run(true)
  },
)
watch(
  () => f.type,
  async (t) => {
    f.base = []
    baseChoices.value = (await fetchFacets(t)).base_models.map((b) => b.name).filter((b) => b !== 'Unknown').slice(0, 12)
  },
)

let io: IntersectionObserver | null = null
onMounted(async () => {
  baseChoices.value = (await fetchFacets(f.type)).base_models.map((b) => b.name).filter((b) => b !== 'Unknown').slice(0, 12)
  io = new IntersectionObserver((entries) => {
    if (entries.some((e) => e.isIntersecting) && items.value.length && cursor.value != null) void run(false)
  }, { rootMargin: '800px' })
  if (sentinel.value) io.observe(sentinel.value)
})
onBeforeUnmount(() => io?.disconnect())

function toggleBase(b: string) {
  f.base = f.base.includes(b) ? f.base.filter((x) => x !== b) : [...f.base, b]
}
function open(card: CivCard) {
  selected.value = card.id
  openOverlay('civitai-model')
}
function markQueued() {
  const c = items.value.find((x) => x.id === selected.value)
  if (c) c.queued = true
}
function setMode(m: Mode) {
  mode.value = m
}

// URL mode
const urlText = ref('')
const urlType = ref<ModelType>('lora')
const urlTarget = ref<Target>({ root: null, folder: '' })
const urlResults = ref<{ input: string; ok: boolean; error?: string }[]>([])
const urlBusy = ref(false)
async function submitUrls() {
  urlBusy.value = true
  try {
    const folder = normalizeFolder(urlTarget.value.folder)
    const r = await enqueueUrls({ text: urlText.value, type: urlType.value, root: urlTarget.value.root, folder })
    urlResults.value = r.results
    rememberTarget(urlType.value, { root: urlTarget.value.root, folder })
    const ok = r.results.filter((x) => x.ok).length
    toast(`${ok} 件をダウンロードに追加しました`)
    if (ok === r.results.length) urlText.value = ''
  } catch (e) {
    toast((e as Error).message, 3000)
  } finally {
    urlBusy.value = false
  }
}
const urlCount = computed(() => urlText.value.split('\n').filter((l) => l.trim()).length)
</script>

<template>
  <section class="civitai">
    <header class="lib-header">
      <div class="seg">
        <button :class="{ active: mode === 'search' }" @click="setMode('search')">検索</button>
        <button :class="{ active: mode === 'favorites' }" @click="setMode('favorites')">Civitai のお気に入り</button>
        <button :class="{ active: mode === 'url' }" @click="setMode('url')">URL から</button>
      </div>
      <template v-if="mode !== 'url'">
        <div class="type-tabs">
          <button v-for="t in TYPES" :key="t.id" :class="{ active: f.type === t.id }" @click="f.type = t.id">{{ t.label }}</button>
        </div>
        <form v-if="mode === 'search'" class="search-row" @submit.prevent="submit">
          <input v-model="f.q" type="search" placeholder="Civitai を検索（キャラ名・作者・タグ）" enterkeyhint="search" />
          <button class="filter-btn on" type="submit">検索</button>
        </form>
        <div class="chips">
          <select v-model="f.sort" class="mini-select"><option v-for="s in SORTS" :key="s" :value="s">{{ SORT_LABEL[s] }}</option></select>
          <select v-model="f.period" class="mini-select"><option v-for="p in PERIODS" :key="p" :value="p">{{ PERIOD_LABEL[p] }}</option></select>
          <label class="chip"><input v-model="f.nsfw" type="checkbox" /> NSFW を含める</label>
        </div>
        <div v-if="baseChoices.length" class="chips">
          <button v-for="b in baseChoices" :key="b" class="chip" :class="{ active: f.base.includes(b) }" @click="toggleBase(b)">{{ b }}</button>
        </div>
      </template>
    </header>

    <div v-if="mode !== 'url'" class="civ-results">
      <p v-if="error" class="err">{{ error }}</p>
      <p v-if="!hasKey && mode === 'search'" class="hint">API キー未設定: 一部のモデルは表示・ダウンロードできません。</p>
      <p v-if="!searched && mode === 'search' && !loading" class="empty">キーワードを入れて検索してください（空欄で検索すると人気順の一覧になります）。</p>
      <div class="civ-grid">
        <button v-for="c in items" :key="c.id" class="card static" @click="open(c)">
          <div class="thumb" :class="{ blur: prefs.blurMature && isMature(c.image?.nsfw_level ?? 0) }">
            <video v-if="c.image && c.image.type === 'video'" :src="c.image.url" muted playsinline loop autoplay preload="metadata" />
            <img v-else-if="c.image" :src="c.image.url" decoding="async" alt="" />
            <div v-else class="noimg">No preview</div>
            <span class="base">{{ c.base_models.join(' / ') || c.type }}</span>
            <span v-if="c.owned_versions" class="owned">所持 {{ c.owned_versions }}/{{ c.versions }}</span>
            <span v-else-if="c.queued" class="owned queued">DL 待ち</span>
          </div>
          <div class="caption">{{ c.name }}</div>
          <div class="sub">♥ {{ c.thumbs_up.toLocaleString() }} ・ ⇩ {{ c.downloads.toLocaleString() }}</div>
        </button>
      </div>
      <div ref="sentinel" class="sentinel"></div>
      <p v-if="loading" class="loading">読み込み中…</p>
      <p v-else-if="searched && !items.length && !error" class="empty">見つかりませんでした</p>
    </div>

    <div v-else class="url-mode">
      <textarea v-model="urlText" rows="5" placeholder="Civitai のモデル URL を 1 行に 1 つずつ貼り付け&#10;例: https://civitai.com/models/12345?modelVersionId=67890"></textarea>
      <div class="type-tabs">
        <button v-for="t in TYPES" :key="t.id" :class="{ active: urlType === t.id }" @click="urlType = t.id">{{ t.label }}</button>
      </div>
      <p class="hint">選んだ種類のモデルは下の保存先へ。違う種類のものは、その種類の最初のルート直下に保存します。</p>
      <TargetPicker v-model="urlTarget" :type="urlType" />
      <div class="actions">
        <button class="primary" :disabled="urlBusy || !urlCount" @click="submitUrls">{{ urlCount }} 件をダウンロード</button>
      </div>
      <ul v-if="urlResults.length" class="url-results">
        <li v-for="r in urlResults" :key="r.input" :class="{ ok: r.ok }">{{ r.ok ? '✓' : '✕' }} <span class="mono">{{ r.input }}</span><br v-if="r.error" /><small v-if="r.error">{{ r.error }}</small></li>
      </ul>
    </div>

    <CivitaiModelSheet v-if="isOpen('civitai-model') && selected != null" :id="selected" @close="closeOverlay" @queued="markQueued" />
  </section>
</template>
