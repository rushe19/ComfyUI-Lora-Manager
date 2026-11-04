<script setup lang="ts">
// Settings: download / restore lmx's own settings (model data needs no backup).
import { ref } from 'vue'
import { toast } from '../toast'
import { loadBaseModels } from '../baseModels'

const busy = ref(false)

async function restore(ev: Event) {
  const input = ev.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  busy.value = true
  try {
    const body = JSON.parse(await file.text())
    const res = await fetch('/api/backup/restore', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) })
    const data = (await res.json()) as { restored?: string[]; message?: string }
    if (!res.ok) throw new Error(data.message ?? `HTTP ${res.status}`)
    await loadBaseModels(true)
    toast(`設定を読み込みました（${(data.restored ?? []).join(', ') || '変更なし'}）`, 3000)
  } catch (e) {
    toast(`読み込めませんでした: ${(e as Error).message}`, 4000)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="card-box">
    <h4>設定のバックアップ</h4>
    <p class="hint">
      Civitai 検索のベースモデル、自動確認の設定、通知しないモデルの一覧を 1 つのファイルに保存します。
      モデルの一覧はスキャンで作り直せるので含みません。lmx.toml は参考として入れます（API キーは伏せ字）。
    </p>
    <div class="actions">
      <a class="button" href="/api/backup" download="lmx-settings.json">ダウンロード</a>
      <label class="button" :class="{ disabled: busy }">
        読み込む<input type="file" accept="application/json,.json" hidden :disabled="busy" @change="restore" />
      </label>
    </div>
  </div>
</template>
