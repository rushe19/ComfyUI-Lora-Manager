<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { fetchUpdateStatus, listDownloads } from './api'
import { auth } from './auth'
import { forge, isEmbedded } from './embed'
import CivitaiView from './components/CivitaiView.vue'
import LoginView from './components/LoginView.vue'
import DownloadsView from './components/DownloadsView.vue'
import LibraryView from './components/LibraryView.vue'
import SettingsView from './components/SettingsView.vue'
import { runToastAction, toastAction, toastText } from './toast'

type Tab = 'library' | 'civitai' | 'downloads' | 'settings'
const tab = ref<Tab>('library')
const civitaiOpened = ref(false)
const activeDownloads = ref(0)
const updates = ref(0)
const tabs: { id: Tab; label: string; icon: string }[] = [
  { id: 'library', label: 'ライブラリ', icon: '▦' },
  { id: 'civitai', label: 'Civitai', icon: '⌕' },
  { id: 'downloads', label: 'DL', icon: '⇩' },
  { id: 'settings', label: '設定', icon: '⚙' },
]

function go(t: Tab) {
  tab.value = t
  if (t === 'civitai') civitaiOpened.value = true
}

// Badge with the number of active downloads.
let timer: number | undefined
async function poll() {
  try {
    activeDownloads.value = (await listDownloads()).jobs.filter((j) => j.status === 'running' || j.status === 'queued').length
  } catch {
    /* ignore */
  }
  timer = window.setTimeout(poll, activeDownloads.value ? 3000 : 15000)
}
onMounted(() => {
  if (!isEmbedded) void poll()
})
onBeforeUnmount(() => window.clearTimeout(timer))

// Badge with the number of owned models that have a new version (checked every 5 minutes).
let upTimer: number | undefined
async function pollUpdates() {
  try {
    const s = await fetchUpdateStatus()
    updates.value = s.counts.models
    upTimer = window.setTimeout(pollUpdates, s.job.running ? 5000 : 300_000)
  } catch {
    upTimer = window.setTimeout(pollUpdates, 300_000)
  }
}
onMounted(() => {
  if (!isEmbedded) void pollUpdates()
})
onBeforeUnmount(() => window.clearTimeout(upTimer))
</script>

<template>
  <LoginView v-if="auth.required" />
  <div v-else class="app">
    <div v-if="isEmbedded" class="forge-bar" title="Forge Neo の現在の状態">
      追加先: <b>{{ forge.tab }}</b><span v-if="forge.checkpoint"> ・ {{ forge.checkpoint }}</span>
    </div>
    <LibraryView v-show="tab === 'library'" class="view" :active="tab === 'library'" />
    <!-- Kept alive after first open so search results survive tab switches. -->
    <CivitaiView v-if="civitaiOpened" v-show="tab === 'civitai'" class="view" />
    <DownloadsView v-if="tab === 'downloads'" class="view" />
    <SettingsView v-if="tab === 'settings'" class="view" />
    <nav v-if="!isEmbedded" class="bottom-nav">
      <button v-for="t in tabs" :key="t.id" :class="{ active: tab === t.id }" @click="go(t.id)">
        <span class="icon">{{ t.icon }}<b v-if="t.id === 'downloads' && activeDownloads" class="nav-badge">{{ activeDownloads }}</b><b v-else-if="t.id === 'library' && updates" class="nav-badge info" title="新しいバージョンがあるモデル">{{ updates > 99 ? '99+' : updates }}</b></span><span>{{ t.label }}</span>
      </button>
    </nav>
    <div v-if="toastText" class="toast" role="status">{{ toastText }}<button v-if="toastAction" class="toast-action" @click="runToastAction">{{ toastAction.label }}</button></div>
  </div>
</template>
