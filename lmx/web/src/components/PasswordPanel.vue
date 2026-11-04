<script setup lang="ts">
// Settings: optional password for the whole app (stored as a hash on the server).
import { onMounted, ref } from 'vue'
import { auth, logout, refreshAuth, setPassword } from '../auth'
import { toast } from '../toast'

const current = ref('')
const next = ref('')
const confirm = ref('')
const busy = ref(false)
const error = ref('')
onMounted(refreshAuth)

async function save(disable = false) {
  error.value = ''
  if (!disable && next.value !== confirm.value) {
    error.value = '確認用のパスワードが一致しません'
    return
  }
  busy.value = true
  const wasEnabled = auth.enabled
  try {
    await setPassword(wasEnabled ? current.value : null, disable ? '' : next.value)
    toast(disable ? 'パスワードを解除しました' : wasEnabled ? 'パスワードを変更しました' : 'パスワードを設定しました')
    current.value = next.value = confirm.value = ''
  } catch (e) {
    error.value = (e as Error).message
  } finally {
    busy.value = false
  }
}
async function doLogout() {
  await logout()
  location.reload()
}
</script>

<template>
  <div class="card-box">
    <h4>パスワード</h4>
    <p class="hint">
      設定すると、ログインしないと一覧・プレビュー・編集などが使えなくなります（ログインは端末ごとに 180 日間有効）。
      今は Tailscale 経由でしか開けないので必須ではありません。LAN のポートを開けるときは設定しておくと安心です。
    </p>
    <p>状態: <strong>{{ auth.enabled ? '設定済み' : '未設定（誰でも開ける）' }}</strong></p>
    <form class="pw-form" @submit.prevent="save()">
      <input v-if="auth.enabled" v-model="current" type="password" autocomplete="current-password" placeholder="今のパスワード" />
      <input v-model="next" type="password" autocomplete="new-password" placeholder="新しいパスワード（6 文字以上）" />
      <input v-model="confirm" type="password" autocomplete="new-password" placeholder="新しいパスワード（確認）" />
      <p v-if="error" class="err-text">{{ error }}</p>
      <div class="actions">
        <button class="primary" type="submit" :disabled="busy || next.length < 6 || (auth.enabled && !current)">{{ auth.enabled ? '変更する' : '設定する' }}</button>
        <button v-if="auth.enabled" type="button" class="danger" :disabled="busy || !current" @click="save(true)">解除する</button>
        <button v-if="auth.enabled" type="button" @click="doLogout">この端末でログアウト</button>
      </div>
    </form>
    <p class="hint">パスワードを忘れたときは、サーバーの <span class="mono">data/auth.json</span> を削除すると解除されます。</p>
  </div>
</template>
