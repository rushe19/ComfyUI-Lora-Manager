<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { fetchHealth, fetchStatus, startScan, type Status } from '../api'
import { prefs, type Theme } from '../prefs'
import { toast } from '../toast'
import { formatDate } from '../util'
import BaseModelSettings from './BaseModelSettings.vue'
import StatsPanel from './StatsPanel.vue'
import UpdatesPanel from './UpdatesPanel.vue'
import MetadataPanel from './MetadataPanel.vue'
import ToolsPanels from './ToolsPanels.vue'

const status = ref<Status | null>(null)
const version = ref('')
const error = ref('')
const secure = window.isSecureContext
const standalone = window.matchMedia('(display-mode: standalone)').matches
let timer: number | undefined

async function refresh() {
  try {
    status.value = await fetchStatus()
    error.value = ''
    if (status.value.scanning) timer = window.setTimeout(refresh, 1000)
  } catch (e) {
    error.value = String(e)
  }
}

async function rescan() {
  const started = await startScan()
  toast(started ? 'スキャンを開始しました' : 'スキャンは実行中です')
  window.clearTimeout(timer)
  timer = window.setTimeout(refresh, 300)
}

onMounted(async () => {
  void refresh()
  try {
    version.value = (await fetchHealth()).version
  } catch {
    /* ignore */
  }
})
onBeforeUnmount(() => window.clearTimeout(timer))

const themes: { id: Theme; label: string }[] = [
  { id: 'system', label: '端末に合わせる' },
  { id: 'dark', label: 'ダーク' },
  { id: 'light', label: 'ライト' },
]
</script>

<template>
  <section class="settings">
    <h2>設定</h2>
    <p v-if="error" class="err">サーバーに接続できません: {{ error }}</p>

    <div class="card-box">
      <h4>表示</h4>
      <div class="seg">
        <button v-for="t in themes" :key="t.id" :class="{ active: prefs.theme === t.id }" @click="prefs.theme = t.id">{{ t.label }}</button>
      </div>
      <label class="check"><input v-model="prefs.blurMature" type="checkbox" /> R 以上のプレビューをぼかす</label>
      <label class="check"><input v-model="prefs.showVideos" type="checkbox" /> 一覧で動画プレビューを再生する</label>
    </div>

    <div v-if="status" class="card-box">
      <h4>ライブラリ</h4>
      <p>
        合計 {{ status.total.toLocaleString() }} 件（LoRA {{ status.counts.lora ?? 0 }} / Checkpoint {{ status.counts.checkpoint ?? 0 }} /
        Embedding {{ status.counts.embedding ?? 0 }} / その他 {{ status.counts.other ?? 0 }}）
      </p>
      <p v-if="status.scanning">スキャン中… {{ status.progress.parsed }} / {{ status.progress.to_parse }}（{{ status.progress.walked_files }} ファイル確認）</p>
      <p v-else-if="status.last_scan">
        前回のスキャン: {{ formatDate(status.last_scan.finished_at) }}、{{ status.last_scan.stats.total_secs.toFixed(1) }} 秒
        （読み直し {{ status.last_scan.stats.parsed }} / 変更なし {{ status.last_scan.stats.unchanged }} / 削除 {{ status.last_scan.stats.removed }}）
        <br />要修復 {{ status.last_scan.stats.damaged + status.last_scan.stats.sidecar_errors }} 件、メタデータ無し {{ status.last_scan.stats.missing_sidecars }} 件
        <span v-if="status.last_scan.error" class="err"><br />エラー: {{ status.last_scan.error }}</span>
      </p>
      <button :disabled="status.scanning" @click="rescan">再スキャン</button>
    </div>

    <UpdatesPanel />
    <StatsPanel />
    <BaseModelSettings />
    <MetadataPanel />
    <ToolsPanels />

    <div v-if="status" class="card-box">
      <h4>モデルの場所</h4>
      <template v-for="(list, t) in status.roots" :key="t">
        <p v-if="list.length"><strong>{{ t }}</strong><br /><span v-for="r in list" :key="r" class="mono">{{ r }}<br /></span></p>
      </template>
    </div>

    <div class="card-box">
      <h4>このアプリについて</h4>
      <p>lmx v{{ version }}</p>
      <p>安全な接続: {{ secure ? 'はい（PWA としてインストール可能）' : 'いいえ（https で開くとインストールできます）' }}</p>
      <p>表示モード: {{ standalone ? 'アプリ（インストール済み）' : 'ブラウザ' }}</p>
    </div>
  </section>
</template>
