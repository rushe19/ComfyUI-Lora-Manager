<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { fetchHealth, type Health } from './api'

const health = ref<Health | null>(null)
const error = ref<string | null>(null)
const secure = window.isSecureContext
const standalone = window.matchMedia('(display-mode: standalone)').matches

onMounted(async () => {
  try {
    health.value = await fetchHealth()
  } catch (e) {
    error.value = String(e)
  }
})
</script>

<template>
  <main class="shell">
    <header class="top">
      <img src="/icons/icon-192.png" alt="" class="logo" />
      <h1>lmx</h1>
    </header>
    <section class="card">
      <p v-if="health">サーバー: {{ health.status }}（{{ health.name }} v{{ health.version }}）</p>
      <p v-else-if="error" class="err">サーバーに接続できません: {{ error }}</p>
      <p v-else>接続中…</p>
      <p>安全なオリジン: {{ secure ? 'はい（PWA インストール可）' : 'いいえ（https で開いてください）' }}</p>
      <p>表示モード: {{ standalone ? 'アプリ（インストール済み）' : 'ブラウザ' }}</p>
    </section>
    <nav class="bottom">
      <span>ライブラリ</span><span>Civitai</span><span>DL</span><span>設定</span>
    </nav>
  </main>
</template>
