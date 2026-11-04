<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { ApiError, civitaiSearch, enqueueUrls, type CivCard, type CivSearchPage, type ModelType } from '../api'
import { baseModels, loadBaseModels, orderLike } from '../baseModels'
import { closeOverlay, isOpen, openOverlay } from '../overlay'
import { prefs } from '../prefs'
import LazyVideo from './LazyVideo.vue'
import { normalizeFolder, rememberTarget, type Target } from '../targets'
import { toast } from '../toast'
import { accessBadge, formatIsoDate, isMature } from '../util'
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
const SORT_LABEL: Record<string, string> = { '': '関連度（標準）', 'Highest Rated': '評価順', 'Most Downloaded': 'DL 数順', Newest: '新着' }
const PERIODS = ['AllTime', 'Year', 'Month', 'Week', 'Day']
const PERIOD_LABEL: Record<string, string> = { AllTime: '全期間', Year: '1 年', Month: '1 か月', Week: '1 週間', Day: '1 日' }

const f = reactive({
  q: '',
  type: 'lora' as ModelType,
  base: [] as string[],
  sort: localStorage.getItem('lmx.civSort') ?? 'Highest Rated',
  period: 'AllTime',
  nsfw: true,
})
const items = ref<CivCard[]>([])
const cursor = ref<string | number | null>(null)
const loading = ref(false)
const error = ref('')
const hasKey = ref(true)
const searched = ref(false)
const resorted = ref<CivSearchPage['resorted']>(null)
const moreFilter = ref('')
// Chips: the selection from settings plus anything picked ad hoc via ＋ほか.
const baseChoices = computed(() => orderLike(baseModels.all, [...new Set([...baseModels.selected, ...f.base])]))
const moreList = computed(() => {
  const n = moreFilter.value.trim().toLowerCase()
  return baseModels.all.filter((b) => !n || b.toLowerCase().includes(n))
})
// Keyword + explicit sort: the server collects up to 1,000 hits first (slower first page).
const sortingPool = computed(() => loading.value && !items.value.length && mode.value === 'search' && !!f.q.trim() && !!f.sort)
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
    resorted.value = page.resorted
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
    localStorage.setItem('lmx.civSort', f.sort)
    if (mode.value !== 'url' && (searched.value || mode.value === 'favorites')) void run(true)
  },
)

let io: IntersectionObserver | null = null
onMounted(async () => {
  void loadBaseModels()
  io = new IntersectionObserver((entries) => {
    if (entries.some((e) => e.isIntersecting) && items.value.length && cursor.value != null) void run(false)
  }, { rootMargin: '800px' })
  if (sentinel.value) io.observe(sentinel.value)
})
onBeforeUnmount(() => io?.disconnect())

function toggleBase(b: string) {
  f.base = f.base.includes(b) ? f.base.filter((x) => x !== b) : [...f.base, b]
}
function openMore() {
  moreFilter.value = ''
  openOverlay('base-more')
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
          <select v-model="f.sort" class="mini-select"><option value="">{{ SORT_LABEL[''] }}</option><option v-for="s in SORTS" :key="s" :value="s">{{ SORT_LABEL[s] }}</option></select>
          <select v-model="f.period" class="mini-select"><option v-for="p in PERIODS" :key="p" :value="p">{{ PERIOD_LABEL[p] }}</option></select>
          <label class="chip"><input v-model="f.nsfw" type="checkbox" /> NSFW を含める</label>
        </div>
        <div class="chips">
          <button v-for="b in baseChoices" :key="b" class="chip" :class="{ active: f.base.includes(b) }" @click="toggleBase(b)">{{ b }}</button>
          <button class="chip more" @click="openMore">＋ほか</button>
        </div>
      </template>
    </header>

    <div v-if="mode !== 'url'" class="civ-results">
      <p v-if="error" class="err">{{ error }}</p>
      <p v-if="!hasKey && mode === 'search'" class="hint">API キー未設定: 一部のモデルは表示・ダウンロードできません。</p>
      <p v-if="resorted && items.length" class="hint">
        キーワードに合う上位 {{ resorted.pool.toLocaleString() }} 件を{{ SORT_LABEL[f.sort] }}で並べています<template v-if="resorted.capped">（Civitai はキーワード検索を 1,000 件までしか返さないため、それより後ろは対象外。期間で絞ると確実です）</template>
      </p>
      <p v-if="!searched && mode === 'search' && !loading" class="empty">キーワードを入れて検索してください（空欄で検索すると人気順の一覧になります）。</p>
      <div class="civ-grid">
        <button v-for="c in items" :key="c.id" class="card static" @click="open(c)">
          <div class="thumb" :class="{ blur: prefs.blurMature && isMature(c.image?.nsfw_level ?? 0) }">
            <template v-if="c.image && c.image.type === 'video'">
              <LazyVideo v-if="prefs.showVideos" :src="c.image.url" pause-behind-overlay />
              <div v-else class="noimg">▶ 動画</div>
            </template>
            <img v-else-if="c.image" :src="c.image.url" decoding="async" alt="" />
            <div v-else class="noimg">No preview</div>
            <span class="base">{{ c.base_models.join(' / ') || c.type }}</span>
            <span v-if="c.owned_versions" class="owned">所持 {{ c.owned_versions }}/{{ c.versions }}</span>
            <span v-else-if="c.queued" class="owned queued">DL 待ち</span>
            <span v-if="c.access" class="access" :class="c.access.kind" :title="c.free_versions ? '旧バージョンは無料でダウンロードできます' : ''">
              {{ accessBadge(c.access) }}<template v-if="c.free_versions"> ・旧版無料</template>
            </span>
          </div>
          <div class="caption">{{ c.name }}</div>
          <div class="sub">♥ {{ c.thumbs_up.toLocaleString() }} ・ ⇩ {{ c.downloads.toLocaleString() }}<template v-if="c.published_at"> ・ {{ formatIsoDate(c.published_at) }}</template></div>
        </button>
      </div>
      <div ref="sentinel" class="sentinel"></div>
      <p v-if="sortingPool" class="loading">並べ替えのため検索結果をまとめて取得しています（最大 1,000 件・数秒）…</p>
      <p v-else-if="loading" class="loading">読み込み中…</p>
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

    <div v-if="isOpen('base-more')" class="sheet-backdrop" @click.self="closeOverlay">
      <div class="sheet">
        <header class="sheet-head">
          <strong class="title">ベースモデルで絞り込み</strong>
          <button class="close" aria-label="閉じる" @click="closeOverlay">✕</button>
        </header>
        <div class="sheet-body">
          <p class="hint">ここで選んだものは今回の検索だけに使います。いつも表示したいものは 設定 → Civitai 検索のベースモデル で選んでください。</p>
          <input v-model="moreFilter" type="search" placeholder="名前で絞り込み" />
          <div class="chips">
            <button v-for="b in moreList" :key="b" class="chip" :class="{ active: f.base.includes(b) }" @click="toggleBase(b)">
              {{ b }}<small v-if="baseModels.local[b]"> 手元</small>
            </button>
          </div>
          <div class="actions"><button class="primary" @click="closeOverlay">閉じる</button></div>
        </div>
      </div>
    </div>

    <CivitaiModelSheet v-if="isOpen('civitai-model') && selected != null" :id="selected" @close="closeOverlay" @queued="markQueued" />
  </section>
</template>
