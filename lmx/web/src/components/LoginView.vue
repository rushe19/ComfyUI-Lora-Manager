<script setup lang="ts">
// Full-screen login shown when the server asks for the password.
import { ref } from 'vue'
import { login } from '../auth'

const pw = ref('')
const error = ref('')
const busy = ref(false)

async function submit() {
  if (!pw.value) return
  busy.value = true
  error.value = ''
  try {
    await login(pw.value)
    // Reload so every view (and preview image) fetches again with the new session.
    location.reload()
  } catch (e) {
    error.value = (e as Error).message
    busy.value = false
  }
}
</script>

<template>
  <div class="login-screen">
    <form class="card-box login-box" @submit.prevent="submit">
      <h2>lmx</h2>
      <p class="hint">パスワードを入れてください。</p>
      <input v-model="pw" type="password" autocomplete="current-password" placeholder="パスワード" autofocus />
      <p v-if="error" class="err-text">{{ error }}</p>
      <button class="primary" type="submit" :disabled="busy || !pw">{{ busy ? '確認中…' : 'ログイン' }}</button>
    </form>
  </div>
</template>
