<script setup lang="ts">
// Settings: new versions of owned models on Civitai (check, auto check, list).
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { ApiError, cancelUpdateCheck, fetchUpdates, fetchUpdateStatus, ignoreUpdate, setAutoUpdate, startUpdateCheck, type ModelType, type UpdateItem, type UpdateStatus } from '../api'
import { withBase } from '../base'
import { closeOverlay, isOpen, openOverlay } from '../overlay'
import { prefs } from '../prefs'
import { toast } from '../toast'
import { accessBadge, formatDate, formatIsoDate, isMature } from '../util'
import CivitaiModelSheet from './CivitaiModelSheet.vue'

const status = ref<UpdateStatus | null>(null)
const items = ref<UpdateItem[]>([])
const type = ref<ModelType | ''>('')
const onlyFree = ref(false)
const shown = ref(30)
const openId = ref<number | null>(null)
let timer: number | undefined

const job = computed(() => status.value?.job ?? null)
const pct = computed(() => (job.value && job.value.total ? Math.round((job.value.done / job.value.total) * 100) : 0))

async function refresh(withList = true) {
  try {
    status.value = await fetchUpdateStatus()
    if (withList) items.value = (await fetchUpdates(type.value || undefined, onlyFree.value)).items
  } catch (e) {
    toast(String(e), 3000)
  }
  window.clearTimeout(timer)
  if (status.value?.job.running) {
    timer = window.setTimeout(async () => {
      const wasRunning = status.value?.job.running
      await refresh(false)
      if (wasRunning && !status.value?.job.running) await refresh(true)
    }, 1000)
  }
}

async function start() {
  const r = await startUpdateCheck()
  toast(r.started ? '新しいバージョンの確認を始めました' : '確認は実行中です')
  await refresh(false)
}
async function stop() {
  await cancelUpdateCheck()
  toast('確認を止めます')
}
async function toggleAuto() {
  if (!status.value) return
  try {
    const r = await setAutoUpdate(!status.value.auto)
    status.value.auto = r.auto
  } catch (e) {
    toast(e instanceof ApiError ? e.message : String(e), 3000)
  }
}
async function ignoreModel(it: UpdateItem) {
  await ignoreUpdate({ model_id: it.model_id, ignore: true })
  toast('このモデルの新バージョンは通知しません')
  await refresh()
}
function open(it: UpdateItem) {
  openId.value = it.model_id
  openOverlay('update-civ-list')
}
async function reloadList() {
  shown.value = 30
  await refresh()
}

onMounted(() => void refresh())
onBeforeUnmount(() => window.clearTimeout(timer))
</script>

<template>
  <div class="card-box">
    <h4>新しいバージョン</h4>
    <p class="hint">手元のモデルについて、Civitai に同じベースモデルの新しいバージョンが出ていないかを確認します（100 件ずつまとめて問い合わせ）。</p>
    <template v-if="status">
      <p>
        <strong>{{ status.counts.models.toLocaleString() }}</strong> 件のモデルに新バージョンがあります（今すぐ無料で落とせるもの {{ status.counts.free.toLocaleString() }} 件）
        <br /><small>
          前回の確認: {{ status.last_full_check ? formatDate(status.last_full_check) : 'まだ' }} ・ 確認済み {{ status.known_models.toLocaleString() }} 件
          <template v-if="status.gone"> ・ Civitai から消えたもの {{ status.gone.toLocaleString() }} 件</template>
          <template v-if="status.ignored_models || status.ignored_versions"> ・ 無視 {{ status.ignored_models }} モデル / {{ status.ignored_versions }} バージョン</template>
        </small>
      </p>
      <div v-if="job?.running">
        <progress :value="job.done" :max="job.total || 1" style="width: 100%" />
        <p class="hint">{{ job.auto ? '自動確認中' : '確認中' }}… {{ job.done.toLocaleString() }} / {{ job.total.toLocaleString() }}（{{ pct }}%）<template v-if="job.errors"> ・ 失敗 {{ job.errors }}</template></p>
        <button @click="stop">中止</button>
      </div>
      <div v-else class="actions">
        <button class="primary" @click="start">今すぐ確認</button>
        <label class="check"><input type="checkbox" :checked="status.auto" @change="toggleAuto" /> 1 日 1 回自動で確認する</label>
      </div>
      <p v-if="job && !job.running && job.last_error" class="err-text">前回の確認でエラー: {{ job.last_error }}</p>

      <template v-if="status.counts.models">
        <div class="chips">
          <select v-model="type" class="mini-select" @change="reloadList">
            <option value="">すべての種類</option>
            <option value="lora">LoRA</option>
            <option value="checkpoint">Checkpoint</option>
            <option value="embedding">Embedding</option>
            <option value="other">その他</option>
          </select>
          <label class="chip"><input v-model="onlyFree" type="checkbox" @change="reloadList" /> 無料のみ</label>
          <span class="hint">{{ items.length.toLocaleString() }} 件（新しい順）</span>
        </div>
        <ul class="update-list">
          <li v-for="it in items.slice(0, shown)" :key="it.model_id">
            <img v-if="it.local.preview && it.local.preview_kind === 'image'" :src="withBase(it.local.preview)" alt="" loading="lazy" :class="{ blur: prefs.blurMature && isMature(it.local.nsfw_level) }" />
            <span v-else class="noimg" style="width: 44px; height: 56px"></span>
            <div class="info">
              <strong>{{ it.local.name }}</strong>
              <small>
                {{ it.versions[0].name }}（{{ it.versions[0].base_model }}<template v-if="it.versions[0].published_at"> ・ {{ formatIsoDate(it.versions[0].published_at) }}</template>）
                <template v-if="it.versions.length > 1"> ほか {{ it.versions.length - 1 }}</template>
              </small>
              <span v-if="it.versions[0].access" class="badge" :class="it.versions[0].access.kind">{{ accessBadge(it.versions[0].access) }}</span>
            </div>
            <button class="small primary" @click="open(it)">開く</button>
            <button class="small" title="このモデルは通知しない" @click="ignoreModel(it)">無視</button>
          </li>
        </ul>
        <button v-if="items.length > shown" class="link" @click="shown += 50">もっと見る（残り {{ items.length - shown }} 件）</button>
      </template>
    </template>
    <CivitaiModelSheet v-if="isOpen('update-civ-list') && openId != null" :id="openId" @close="closeOverlay" @queued="refresh()" />
  </div>
</template>
