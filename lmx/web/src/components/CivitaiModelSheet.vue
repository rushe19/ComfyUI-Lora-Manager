<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { ApiError, civitaiModel, enqueueDownload, type CivFile, type CivModel, type CivVersion } from '../api'
import { closeOverlay, isOpen, openOverlay } from '../overlay'
import { prefs } from '../prefs'
import { normalizeFolder, rememberTarget, type Target } from '../targets'
import { toast } from '../toast'
import { accessBadge, copyText, formatIsoDate, formatSize, htmlToText, isMature, timeLeft, viewerUrl } from '../util'
import MediaViewer, { type ViewerItem } from './MediaViewer.vue'
import TargetPicker from './TargetPicker.vue'

const props = defineProps<{ id: number }>()
const emit = defineEmits<{ (e: 'close'): void; (e: 'queued'): void }>()

const m = ref<CivModel | null>(null)
const error = ref('')
const vIndex = ref(0)
const picking = ref<{ version: CivVersion; file: CivFile } | null>(null)
const target = ref<Target>({ root: null, folder: '' })
const busy = ref(false)
const showDesc = ref(false)

const version = computed(() => m.value?.versions[vIndex.value] ?? null)
const description = computed(() => htmlToText(version.value?.description || m.value?.description || ''))

async function load() {
  try {
    m.value = await civitaiModel(props.id)
    error.value = ''
  } catch (e) {
    error.value = (e as Error).message
  }
}
onMounted(load)

function startDownload(file: CivFile) {
  if (!version.value) return
  picking.value = { version: version.value, file }
  openOverlay('picker')
}
// The Android back button closes the picker through the overlay stack.
watch(
  () => isOpen('picker'),
  (open) => {
    if (!open) picking.value = null
  },
)

async function confirm() {
  if (!picking.value || !m.value) return
  busy.value = true
  const folder = normalizeFolder(target.value.folder)
  try {
    await enqueueDownload({ model_id: m.value.id, version_id: picking.value.version.id, file_id: picking.value.file.id, root: target.value.root, folder })
    rememberTarget(m.value.lmx_type, { root: target.value.root, folder })
    toast('ダウンロードに追加しました')
    closeOverlay()
    emit('queued')
    await load()
  } catch (e) {
    toast(e instanceof ApiError ? e.message : String(e), 3000)
  } finally {
    busy.value = false
  }
}

const viewer = ref<{ items: ViewerItem[]; start: number } | null>(null)
function openViewer(start: number) {
  const imgs = version.value?.images ?? []
  viewer.value = {
    items: imgs.map((i) => ({ url: viewerUrl(i.full || i.url, i.type === 'video'), video: i.type === 'video', nsfw: i.nsfw_level })),
    start,
  }
  openOverlay('viewer-civ')
}
async function copyCreator(name: string) {
  toast((await copyText(name)) ? '作者名をコピーしました' : 'コピーできませんでした')
}

const jobLabel: Record<string, string> = { queued: '待機中', running: 'ダウンロード中', done: 'DL 済み', failed: '失敗', canceled: '取消' }
</script>

<template>
  <div class="sheet-backdrop" @click.self="emit('close')">
    <article class="sheet detail">
      <header class="sheet-head">
        <strong class="title">{{ m?.name ?? '読み込み中…' }}</strong>
        <button class="close" aria-label="閉じる" @click="emit('close')">✕</button>
      </header>
      <div class="sheet-body">
        <p v-if="error" class="err">{{ error }}</p>
        <template v-if="m">
          <div class="meta-chips">
            <span class="chip">{{ m.type }}</span>
            <button v-if="m.creator" class="chip" title="タップで作者名をコピー" @click="copyCreator(m.creator)">by {{ m.creator }}</button>
            <span v-if="m.nsfw" class="chip warn">NSFW</span>
            <span v-if="m.first_published_at" class="chip">初公開 {{ formatIsoDate(m.first_published_at) }}</span>
            <span v-if="m.last_published_at && formatIsoDate(m.last_published_at) !== formatIsoDate(m.first_published_at)" class="chip">最終更新 {{ formatIsoDate(m.last_published_at) }}</span>
            <a class="chip" :href="m.url" target="_blank" rel="noopener noreferrer">Civitai で開く ↗</a>
          </div>

          <h4>バージョン</h4>
          <div class="seg versions">
            <button v-for="(v, i) in m.versions" :key="v.id" :class="{ active: i === vIndex }" @click="vIndex = i">
              {{ v.name }}<small> {{ v.base_model }}</small>
              <span v-if="v.owned_local_id" class="badge ok">所持</span>
              <span v-else-if="v.job" class="badge">{{ jobLabel[v.job] ?? v.job }}</span>
              <span v-else-if="v.access" class="badge" :class="v.access.kind">{{ accessBadge(v.access) }}</span>
            </button>
          </div>

          <template v-if="version">
            <div v-if="version.images.length" class="strip">
              <button v-for="(img, i) in version.images" :key="img.url" class="strip-item" :aria-label="`サンプル ${i + 1} を拡大`" @click="openViewer(i)">
                <video v-if="img.type === 'video'" :src="img.url" muted playsinline loop preload="metadata" :class="{ blur: prefs.blurMature && isMature(img.nsfw_level) }" />
                <img v-else :src="img.url" alt="" :class="{ blur: prefs.blurMature && isMature(img.nsfw_level) }" />
                <span v-if="img.type === 'video'" class="play-badge">▶</span>
              </button>
            </div>

            <p class="hint">公開日 {{ formatIsoDate(version.published_at, true) || '不明' }}<template v-if="version.downloads"> ・ ⇩ {{ version.downloads.toLocaleString() }}</template></p>
            <p v-if="version.owned_local_id" class="ok-box">このバージョンはライブラリにあります。</p>
            <p v-else-if="version.access?.kind === 'early'" class="warn-box">
              先行公開中です。{{ formatIsoDate(version.access.ends_at, true) }} まで（{{ timeLeft(version.access.ends_at) }}）はダウンロードに Buzz での購入が必要で、その後は無料になります。Civitai で購入済みならそのままダウンロードできます。
            </p>
            <p v-else-if="version.access?.kind === 'paid'" class="warn-box">
              このバージョンのダウンロードには Buzz での購入が必要です（期限なし。価格は Civitai のページで確認してください）。Civitai で購入済みならそのままダウンロードできます。
            </p>
            <p v-if="version.availability && version.availability !== 'Public'" class="warn-box">公開状態: {{ version.availability }}（API キーやアクセス権が必要な場合があります）</p>

            <h4>ファイル</h4>
            <ul class="files">
              <li v-for="f in version.files" :key="f.id">
                <div class="file-info">
                  <strong>{{ f.name }}</strong>
                  <small>{{ f.type }} ・ {{ formatSize(f.size_kb * 1024) }}<template v-if="f.fp"> ・ {{ f.fp }}</template><template v-if="f.primary"> ・ 推奨</template></small>
                </div>
                <span v-if="f.owned_local_id" class="badge ok">所持</span>
                <button v-else class="primary small" :disabled="!!version.job && ['queued', 'running'].includes(version.job)" @click="startDownload(f)">ダウンロード</button>
              </li>
            </ul>

            <section v-if="version.trained_words.length">
              <h4>トリガーワード</h4>
              <div class="chips"><span v-for="w in version.trained_words" :key="w" class="chip word">{{ w }}</span></div>
            </section>

            <section v-if="m.tags.length">
              <h4>タグ</h4>
              <div class="chips"><span v-for="t in m.tags" :key="t" class="chip">{{ t }}</span></div>
            </section>

            <section v-if="description">
              <h4>説明</h4>
              <p class="pre" :class="{ clamp: !showDesc }">{{ description }}</p>
              <button class="link" @click="showDesc = !showDesc">{{ showDesc ? '閉じる' : 'もっと見る' }}</button>
            </section>
          </template>
        </template>
      </div>

      <div v-if="picking && m" class="picker-panel">
        <header class="sheet-head">
          <strong class="title">保存先を選ぶ</strong>
          <button class="close" aria-label="閉じる" @click="closeOverlay">✕</button>
        </header>
        <div class="sheet-body">
          <p class="mono">{{ picking.file.name }}</p>
          <TargetPicker v-model="target" :type="m.lmx_type" :base-model="picking.version.base_model" />
          <div class="actions">
            <button class="primary" :disabled="busy" @click="confirm">ここにダウンロード</button>
          </div>
        </div>
      </div>
    </article>
    <MediaViewer v-if="viewer && isOpen('viewer-civ')" :items="viewer.items" :start="viewer.start" @close="closeOverlay" />
  </div>
</template>
