<script setup lang="ts">
// Bulk metadata creation: local archive first / only / Civitai first (利用者要望).
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { ApiError, cancelMetadata, metadataJob, metadataPreview, startMetadata, type MetaJob, type MetaPreview, type MetadataMode, type ModelType } from '../api'
import { toast } from '../toast'

const MODES: { id: MetadataMode; label: string; hint: string }[] = [
  { id: 'archive_first', label: 'ローカル DB 優先', hint: 'アーカイブ DB にあるものはそれで作り、無いものだけ Civitai に聞きます（おすすめ）' },
  { id: 'archive_only', label: 'ローカル DB のみ', hint: 'Civitai には一切問い合わせません。大量処理やオフライン向け（プレビュー画像も取りません）' },
  { id: 'civitai_first', label: 'Civitai 優先', hint: '最新情報を優先。見つからないときだけアーカイブ DB を使います（時間がかかります）' },
]
const TYPES: { id: ModelType; label: string }[] = [
  { id: 'lora', label: 'LoRA' },
  { id: 'checkpoint', label: 'Checkpoint' },
  { id: 'embedding', label: 'Embedding' },
  { id: 'other', label: 'その他' },
]
const form = reactive({ type: 'lora' as ModelType, mode: 'archive_first' as MetadataMode, previews: true, retry: false })
const preview = ref<MetaPreview | null>(null)
const job = ref<MetaJob | null>(null)
const loadingPreview = ref(false)
let timer: number | undefined

async function loadPreview() {
  loadingPreview.value = true
  try {
    preview.value = await metadataPreview({ type: form.type })
  } catch (e) {
    toast(String(e), 3000)
  } finally {
    loadingPreview.value = false
  }
}
async function poll() {
  try {
    job.value = (await metadataJob()).job
  } catch {
    /* ignore */
  }
  window.clearTimeout(timer)
  if (job.value?.running) timer = window.setTimeout(poll, 700)
  else if (job.value) void loadPreview()
}
onMounted(async () => {
  await poll()
  if (preview.value === null) void loadPreview()
})
onBeforeUnmount(() => window.clearTimeout(timer))
watch(() => form.type, loadPreview)

async function start() {
  try {
    job.value = (await startMetadata({ type: form.type, mode: form.mode, only_unlinked: true, previews: form.previews, retry_misses: form.retry })).job
    toast('メタデータ取得を始めました')
    timer = window.setTimeout(poll, 500)
  } catch (e) {
    toast(e instanceof ApiError ? e.message : String(e), 4000)
  }
}
async function cancel() {
  await cancelMetadata()
  toast('止めています…')
}
const pct = computed(() => (job.value && job.value.total ? Math.round((job.value.done / job.value.total) * 100) : 0))
const modeLabel = (m: MetadataMode | null) => MODES.find((x) => x.id === m)?.label ?? m
</script>

<template>
  <div class="card-box meta-panel">
    <h4>メタデータの一括取得</h4>
    <p class="hint">Civitai 情報が無いモデルの .metadata.json を作ります。書き換える前の内容は data/backups に保存されます。</p>
    <div class="type-tabs">
      <button v-for="t in TYPES" :key="t.id" :class="{ active: form.type === t.id }" @click="form.type = t.id">{{ t.label }}</button>
    </div>
    <p v-if="preview">
      対象 {{ preview.total.toLocaleString() }} 件<template v-if="preview.archive">（うちローカル DB にあり {{ preview.in_archive.toLocaleString() }} 件<template v-if="preview.no_hash">、ハッシュ未計算 {{ preview.no_hash }} 件</template>）</template>
      <br v-if="!preview.archive" /><span v-if="!preview.archive" class="err-text">ローカル DB（archive_db）が見つかりません</span>
    </p>
    <p v-else-if="loadingPreview">数えています…</p>
    <div class="mode-list">
      <label v-for="m in MODES" :key="m.id" class="mode" :class="{ active: form.mode === m.id }">
        <input v-model="form.mode" type="radio" :value="m.id" />
        <span><strong>{{ m.label }}</strong><small>{{ m.hint }}</small></span>
      </label>
    </div>
    <label class="check"><input v-model="form.previews" type="checkbox" :disabled="form.mode === 'archive_only'" /> プレビューが無いモデルは画像も取る</label>
    <label class="check"><input v-model="form.retry" type="checkbox" /> 最近 Civitai で見つからなかったものも聞き直す</label>
    <div class="actions">
      <button class="primary" :disabled="job?.running || !preview?.total" @click="start">開始</button>
      <button v-if="job?.running" @click="cancel">中止</button>
    </div>

    <div v-if="job" class="job-box">
      <p>
        <strong>{{ job.running ? '実行中' : job.canceled ? '中止しました' : '完了' }}</strong>（{{ modeLabel(job.mode) }}）
        {{ job.done.toLocaleString() }} / {{ job.total.toLocaleString() }} ・ {{ job.elapsed_secs.toFixed(1) }} 秒
      </p>
      <div class="bar"><span :style="{ width: pct + '%' }"></span></div>
      <p v-if="job.running && job.current" class="hint mono">{{ job.current }}</p>
      <ul class="stats">
        <li>ローカル DB で解決 <b>{{ job.archive_hits }}</b></li>
        <li>Civitai で解決 <b>{{ job.civitai_hits }}</b></li>
        <li>見つからず <b>{{ job.unresolved }}</b></li>
        <li>スキップ <b>{{ job.skipped }}</b></li>
        <li>エラー <b>{{ job.errors }}</b></li>
        <li>Civitai API 呼び出し <b>{{ job.api_requests }}</b></li>
        <li v-if="job.previews_added">プレビュー追加 <b>{{ job.previews_added }}</b></li>
        <li v-if="job.hashed">ハッシュ計算 <b>{{ job.hashed }}</b></li>
      </ul>
      <p v-if="job.backup_dir" class="hint mono">バックアップ: {{ job.backup_dir }}</p>
      <details v-if="job.recent_errors.length"><summary>エラーの内容</summary><p v-for="e in job.recent_errors" :key="e" class="hint mono">{{ e }}</p></details>
    </div>
  </div>
</template>
