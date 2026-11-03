<script setup lang="ts">
import { ref } from 'vue'
import LibraryView from './components/LibraryView.vue'
import SettingsView from './components/SettingsView.vue'
import { toastText } from './toast'

type Tab = 'library' | 'civitai' | 'downloads' | 'settings'
const tab = ref<Tab>('library')
const tabs: { id: Tab; label: string; icon: string }[] = [
  { id: 'library', label: 'ライブラリ', icon: '▦' },
  { id: 'civitai', label: 'Civitai', icon: '⌕' },
  { id: 'downloads', label: 'DL', icon: '⇩' },
  { id: 'settings', label: '設定', icon: '⚙' },
]
</script>

<template>
  <div class="app">
    <LibraryView v-show="tab === 'library'" class="view" />
    <section v-if="tab === 'civitai' || tab === 'downloads'" class="view placeholder">
      <p>{{ tab === 'civitai' ? 'Civitai 検索' : 'ダウンロード' }}は Phase 2 で追加予定です。</p>
    </section>
    <SettingsView v-if="tab === 'settings'" class="view" />
    <nav class="bottom-nav">
      <button v-for="t in tabs" :key="t.id" :class="{ active: tab === t.id }" @click="tab = t.id">
        <span class="icon">{{ t.icon }}</span><span>{{ t.label }}</span>
      </button>
    </nav>
    <div v-if="toastText" class="toast" role="status">{{ toastText }}</div>
  </div>
</template>
