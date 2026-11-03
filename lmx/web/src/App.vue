<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { listDownloads } from './api'
import CivitaiView from './components/CivitaiView.vue'
import DownloadsView from './components/DownloadsView.vue'
import LibraryView from './components/LibraryView.vue'
import SettingsView from './components/SettingsView.vue'
import { runToastAction, toastAction, toastText } from './toast'

type Tab = 'library' | 'civitai' | 'downloads' | 'settings'
const tab = ref<Tab>('library')
const civitaiOpened = ref(false)
const activeDownloads = ref(0)
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
onMounted(poll)
onBeforeUnmount(() => window.clearTimeout(timer))
</script>

<template>
  <div class="app">
    <LibraryView v-show="tab === 'library'" class="view" :active="tab === 'library'" />
    <!-- Kept alive after first open so search results survive tab switches. -->
    <CivitaiView v-if="civitaiOpened" v-show="tab === 'civitai'" class="view" />
    <DownloadsView v-if="tab === 'downloads'" class="view" />
    <SettingsView v-if="tab === 'settings'" class="view" />
    <nav class="bottom-nav">
      <button v-for="t in tabs" :key="t.id" :class="{ active: tab === t.id }" @click="go(t.id)">
        <span class="icon">{{ t.icon }}<b v-if="t.id === 'downloads' && activeDownloads" class="nav-badge">{{ activeDownloads }}</b></span><span>{{ t.label }}</span>
      </button>
    </nav>
    <div v-if="toastText" class="toast" role="status">{{ toastText }}<button v-if="toastAction" class="toast-action" @click="runToastAction">{{ toastAction.label }}</button></div>
  </div>
</template>
