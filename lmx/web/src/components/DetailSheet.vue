<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { ApiError, editModel, fetchDetail, ignoreUpdate, metadataJob, startMetadata, type ModelDetail } from '../api'
import { trashWithUndo } from '../actions'
import { closeOverlay, isOpen, openOverlay } from '../overlay'
import { prefs } from '../prefs'
import { toast } from '../toast'
import { accessBadge, copyText, formatDate, formatIsoDate, formatSize, htmlToText, isMature, thumbUrl, viewerUrl } from '../util'
import CivitaiModelSheet from './CivitaiModelSheet.vue'
import EditPanel from './EditPanel.vue'
import LazyVideo from './LazyVideo.vue'
import MediaViewer, { type ViewerItem } from './MediaViewer.vue'
import MovePanel from './MovePanel.vue'

const props = defineProps<{ id: number }>()
const emit = defineEmits<{ (e: 'close'): void; (e: 'changed'): void; (e: 'reopen', id: number): void }>()
const fetching = ref(false)
const confirmDelete = ref(false)

async function reload() {
  d.value = await fetchDetail(props.id)
}
watch(() => props.id, () => void reload())

async function toggleFavorite() {
  if (!d.value) return
  try {
    d.value = await editModel(d.value.summary.id, { favorite: !d.value.summary.favorite })
    emit('changed')
  } catch (e) {
    toast(e instanceof ApiError ? e.message : String(e), 3000)
  }
}

async function fetchMeta() {
  if (!d.value) return
  fetching.value = true
  try {
    await startMetadata({ ids: [d.value.summary.id], only_unlinked: false })
    for (let i = 0; i < 300; i++) {
      await new Promise((r) => setTimeout(r, 400))
      const j = (await metadataJob()).job
      if (j && !j.running) {
        toast(j.archive_hits ? 'アーカイブ DB から取得しました' : j.civitai_hits ? 'Civitai から取得しました' : j.skipped ? '取得をスキップしました（除外設定か破損）' : '見つかりませんでした', 3000)
        break
      }
    }
    await reload()
    emit('changed')
  } catch (e) {
    toast(e instanceof ApiError ? e.message : String(e), 3000)
  } finally {
    fetching.value = false
  }
}

async function doDelete() {
  if (!d.value) return
  confirmDelete.value = false
  if (await trashWithUndo(d.value.summary.id, () => emit('changed'))) emit('close')
}

function onSaved(nd: ModelDetail) {
  d.value = nd
  closeOverlay()
  emit('changed')
}
function onMoved(newId: number | null) {
  closeOverlay()
  emit('changed')
  if (newId != null && newId !== props.id) emit('reopen', newId)
  else void reload()
}

const d = ref<ModelDetail | null>(null)
const error = ref('')
const showDesc = ref(false)
const description = computed(() => (d.value ? htmlToText(d.value.model_description || d.value.version_description) : ''))
const preview = computed(() => d.value?.summary.preview ?? null)

onMounted(async () => {
  try {
    d.value = await fetchDetail(props.id)
  } catch (e) {
    error.value = String(e)
  }
})

async function ignore(body: { model_id?: number; version_id?: number }, label: string) {
  try {
    await ignoreUpdate({ ...body, ignore: true })
    toast(label)
    await reload()
    emit('changed')
  } catch (e) {
    toast(e instanceof ApiError ? e.message : String(e), 3000)
  }
}

const viewer = ref<{ items: ViewerItem[]; start: number } | null>(null)
function openViewer(start: number) {
  if (!d.value) return
  viewer.value = {
    items: d.value.images.map((i) => ({ url: viewerUrl(i.url, i.type === 'video'), video: i.type === 'video', nsfw: i.nsfw_level ?? 0 })),
    start,
  }
  openOverlay('viewer-detail')
}
async function copy(text: string, label: string) {
  toast((await copyText(text)) ? `${label}をコピーしました` : 'コピーできませんでした')
}
</script>

<template>
  <div class="sheet-backdrop" @click.self="emit('close')">
    <article class="sheet detail">
      <header class="sheet-head">
        <strong class="title">{{ d?.summary.name ?? '読み込み中…' }}</strong>
        <button class="close" aria-label="閉じる" @click="emit('close')">✕</button>
      </header>
      <div class="sheet-body">
        <p v-if="error" class="err">{{ error }}</p>
        <template v-if="d">
          <div class="hero" :class="{ blur: prefs.blurMature && isMature(d.summary.nsfw_level) }">
            <video v-if="preview && d.summary.preview_kind === 'video'" :src="preview" controls muted playsinline loop autoplay />
            <img v-else-if="preview" :src="preview" alt="" />
          </div>

          <p v-if="d.damage || d.sidecar_error" class="warn-box">
            メタデータ（.metadata.json）が壊れています。{{ d.damage ? '読める部分だけ表示しています。' : d.sidecar_error }}
          </p>

          <div class="meta-chips">
            <span class="chip">{{ d.summary.base_model }}</span>
            <span v-if="d.version_name" class="chip">{{ d.version_name }}</span>
            <span class="chip">{{ formatSize(d.summary.size) }}</span>
            <span class="chip">{{ formatDate(d.summary.modified) }}</span>
            <button v-if="d.creator" class="chip" title="タップで作者名をコピー" @click="copy(d.creator, '作者名')">by {{ d.creator }}</button>
          </div>

          <div class="actions">
            <button v-if="d.lora_syntax" class="primary" @click="copy(d.lora_syntax, 'LoRA タグ')">{{ d.lora_syntax }} をコピー</button>
            <button v-if="d.trained_words.length" @click="copy(d.trained_words.join(', '), 'トリガーワード')">トリガーワードを全部コピー</button>
            <a v-if="d.civitai_url" class="button" :href="d.civitai_url" target="_blank" rel="noopener noreferrer">Civitai で開く ↗</a>
          </div>
          <div class="actions tools">
            <button :class="{ on: d.summary.favorite }" @click="toggleFavorite">{{ d.summary.favorite ? '★ お気に入り' : '☆ お気に入り' }}</button>
            <button @click="openOverlay('edit')">✎ 編集</button>
            <button @click="openOverlay('move')">⇄ 移動・改名</button>
            <button :disabled="fetching" @click="fetchMeta">{{ fetching ? '取得中…' : d.summary.linked ? '↻ メタデータ更新' : '⤓ メタデータ取得' }}</button>
            <button class="danger" @click="confirmDelete = true">🗑 削除</button>
          </div>
          <p v-if="confirmDelete" class="warn-box">
            「{{ d.summary.name }}」と付属ファイルを Windows のゴミ箱へ移動します。
            <span class="actions"><button class="danger" @click="doDelete">ゴミ箱へ移動</button><button @click="confirmDelete = false">やめる</button></span>
          </p>

          <section v-if="d.new_versions.length" class="card-box">
            <h4>Civitai に新しいバージョンがあります</h4>
            <ul class="new-versions">
              <li v-for="v in d.new_versions" :key="v.id">
                <span class="grow"><strong>{{ v.name }}</strong> <small>{{ v.base_model }}<template v-if="v.published_at"> ・ {{ formatIsoDate(v.published_at) }}</template></small></span>
                <span v-if="v.access" class="badge" :class="v.access.kind">{{ accessBadge(v.access) }}</span>
                <button class="small" @click="ignore({ version_id: v.id }, 'このバージョンは通知しません')">無視</button>
              </li>
            </ul>
            <div class="actions">
              <button v-if="d.civitai_model_id" class="primary" @click="openOverlay('update-civ')">ダウンロード画面を開く</button>
              <button v-if="d.civitai_model_id" @click="ignore({ model_id: d.civitai_model_id }, 'このモデルの新バージョンは通知しません')">このモデルは通知しない</button>
            </div>
          </section>

          <section v-if="d.trained_words.length">
            <h4>トリガーワード <small>（タップでコピー）</small></h4>
            <div class="chips">
              <button v-for="w in d.trained_words" :key="w" class="chip word" @click="copy(w, `「${w}」`)">{{ w }}</button>
            </div>
          </section>

          <section v-if="d.tags.length">
            <h4>タグ</h4>
            <div class="chips"><span v-for="t in d.tags" :key="t" class="chip">{{ t }}</span></div>
          </section>

          <section v-if="d.notes">
            <h4>メモ</h4>
            <p class="pre">{{ d.notes }}</p>
          </section>

          <section v-if="description">
            <h4>説明</h4>
            <p class="pre" :class="{ clamp: !showDesc }">{{ description }}</p>
            <button class="link" @click="showDesc = !showDesc">{{ showDesc ? '閉じる' : 'もっと見る' }}</button>
          </section>

          <section v-if="d.images.length">
            <h4>Civitai のサンプル画像 <small>（タップで拡大）</small></h4>
            <div class="strip">
              <button v-for="(img, i) in d.images" :key="img.url" class="strip-item" :aria-label="`サンプル ${i + 1} を拡大`" @click="openViewer(i)">
                <LazyVideo v-if="img.type === 'video'" :src="thumbUrl(img.url, true)" :style="{ aspectRatio: (img.width && img.height) ? `${img.width} / ${img.height}` : '3 / 4' }" :class="{ blur: prefs.blurMature && isMature(img.nsfw_level ?? 0) }" />
                <img v-else :src="thumbUrl(img.url, false)" loading="lazy" decoding="async" alt="" :class="{ blur: prefs.blurMature && isMature(img.nsfw_level ?? 0) }" />
                <span v-if="img.type === 'video'" class="play-badge">▶</span>
              </button>
            </div>
          </section>

          <section>
            <h4>ファイル</h4>
            <dl class="kv">
              <dt>場所</dt>
              <dd class="mono" @click="copy(d.file_path, 'パス')">{{ d.file_path }}</dd>
              <dt>SHA256</dt>
              <dd class="mono" @click="d.sha256 && copy(d.sha256, 'ハッシュ')">{{ d.sha256 ?? '未計算' }}</dd>
              <dt>取得元</dt>
              <dd>{{ d.metadata_source ?? '未取得' }}</dd>
            </dl>
          </section>
        </template>
      </div>
      <EditPanel v-if="d && isOpen('edit')" :d="d" @saved="onSaved" @close="closeOverlay" />
      <MovePanel v-if="d && isOpen('move')" :d="d" @moved="onMoved" @close="closeOverlay" />
    </article>
    <MediaViewer v-if="viewer && isOpen('viewer-detail')" :items="viewer.items" :start="viewer.start" @close="closeOverlay" />
    <CivitaiModelSheet v-if="isOpen('update-civ') && d?.civitai_model_id" :id="d.civitai_model_id" @close="closeOverlay" @queued="emit('changed')" />
  </div>
</template>
