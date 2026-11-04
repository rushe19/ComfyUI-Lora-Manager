<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { cancelDownload, listDownloads, retryDownload, type DownloadJob } from '../api'
import { prefs } from '../prefs'
import { toast } from '../toast'
import { formatSize, isMature, isVideoUrl } from '../util'
import LazyVideo from './LazyVideo.vue'

const jobs = ref<DownloadJob[]>([])
const error = ref('')
const speeds = ref<Record<number, number>>({})
let last: Record<number, { bytes: number; t: number }> = {}
let timer: number | undefined

async function refresh() {
  try {
    const list = (await listDownloads()).jobs
    const now = performance.now()
    const next: Record<number, { bytes: number; t: number }> = {}
    const sp: Record<number, number> = {}
    for (const j of list) {
      next[j.id] = { bytes: j.bytes_done, t: now }
      const prev = last[j.id]
      if (j.status === 'running' && prev && now > prev.t) sp[j.id] = ((j.bytes_done - prev.bytes) * 1000) / (now - prev.t)
    }
    last = next
    speeds.value = sp
    jobs.value = list
    error.value = ''
  } catch (e) {
    error.value = String(e)
  }
  const active = jobs.value.some((j) => j.status === 'running' || j.status === 'queued')
  timer = window.setTimeout(refresh, active ? 1000 : 5000)
}

onMounted(refresh)
onBeforeUnmount(() => window.clearTimeout(timer))

async function cancel(j: DownloadJob) {
  const r = await cancelDownload(j.id)
  toast(r.result === 'removed' ? '一覧から消しました' : 'キャンセルしました')
  window.clearTimeout(timer)
  void refresh()
}
async function retry(j: DownloadJob) {
  await retryDownload(j.id)
  toast('もう一度ダウンロードします')
  window.clearTimeout(timer)
  void refresh()
}

const label: Record<string, string> = { queued: '待機中', running: 'ダウンロード中', done: '完了', failed: '失敗', canceled: 'キャンセル' }
const active = computed(() => jobs.value.filter((j) => j.status === 'running' || j.status === 'queued'))
const finished = computed(() => jobs.value.filter((j) => j.status !== 'running' && j.status !== 'queued'))
const pct = (j: DownloadJob) => (j.size_bytes ? Math.min(100, Math.round((j.bytes_done / j.size_bytes) * 100)) : 0)
</script>

<template>
  <section class="downloads">
    <h2>ダウンロード</h2>
    <p v-if="error" class="err">{{ error }}</p>
    <p v-if="!jobs.length" class="empty">ダウンロードはまだありません。Civitai タブから追加できます。</p>
    <template v-for="group in [{ title: '進行中', list: active }, { title: '履歴', list: finished }]" :key="group.title">
      <h4 v-if="group.list.length">{{ group.title }}</h4>
      <ul class="jobs">
        <li v-for="j in group.list" :key="j.id" :class="j.status">
          <LazyVideo v-if="j.image_url && isVideoUrl(j.image_url)" :src="j.image_url" pause-behind-overlay :class="{ blur: prefs.blurMature && isMature(j.nsfw_level) }" />
          <img v-else-if="j.image_url" :src="j.image_url" alt="" :class="{ blur: prefs.blurMature && isMature(j.nsfw_level) }" />
          <div class="job-body">
            <strong>{{ j.model_name }}</strong>
            <small>{{ j.version_name }} ・ {{ j.base_model }} ・ {{ label[j.status] }}</small>
            <div v-if="j.status === 'running' || j.status === 'queued'" class="bar"><span :style="{ width: pct(j) + '%' }"></span></div>
            <small v-if="j.status === 'running'">{{ formatSize(j.bytes_done) }} / {{ formatSize(j.size_bytes) }}（{{ pct(j) }}%）<template v-if="speeds[j.id]"> ・ {{ formatSize(speeds[j.id]) }}/s</template></small>
            <small class="mono">{{ j.result_path ?? `${j.dest_dir}/${j.file_name}` }}</small>
            <small v-if="j.error" class="err-text">{{ j.error }}</small>
          </div>
          <div class="job-actions">
            <button v-if="j.status === 'failed' || j.status === 'canceled'" @click="retry(j)">再試行</button>
            <button @click="cancel(j)">{{ j.status === 'running' || j.status === 'queued' ? '中止' : '消す' }}</button>
          </div>
        </li>
      </ul>
    </template>
  </section>
</template>
