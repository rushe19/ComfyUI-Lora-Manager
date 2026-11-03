<script setup lang="ts">
// Same-hash files (duplicates) and the Recycle Bin history with restore.
import { onMounted, ref } from 'vue'
import { fetchDuplicates, listTrash, type DupGroup, type ModelType, type TrashEntry } from '../api'
import { trashWithUndo, undoTrash } from '../actions'
import { formatDate, formatSize } from '../util'

const type = ref<ModelType>('lora')
const groups = ref<DupGroup[] | null>(null)
const wasted = ref(0)
const trash = ref<TrashEntry[]>([])
const loading = ref(false)

async function loadDups() {
  loading.value = true
  try {
    const r = await fetchDuplicates(type.value)
    groups.value = r.groups
    wasted.value = r.wasted_bytes
  } finally {
    loading.value = false
  }
}
async function loadTrash() {
  trash.value = (await listTrash()).items.slice(0, 50)
}
onMounted(loadTrash)

async function remove(id: number) {
  await trashWithUndo(id, () => {
    void loadDups()
    void loadTrash()
  })
}
async function restore(e: TrashEntry) {
  await undoTrash(e, () => void loadTrash())
}
</script>

<template>
  <div class="card-box">
    <h4>重複ファイル（同じ SHA256）</h4>
    <div class="actions">
      <select v-model="type" class="mini-select">
        <option value="lora">LoRA</option>
        <option value="checkpoint">Checkpoint</option>
        <option value="embedding">Embedding</option>
        <option value="other">その他</option>
      </select>
      <button :disabled="loading" @click="loadDups">{{ loading ? '探しています…' : '探す' }}</button>
    </div>
    <template v-if="groups">
      <p>{{ groups.length }} 組、削れる容量 {{ formatSize(wasted) }}</p>
      <div v-for="g in groups.slice(0, 100)" :key="g.sha256" class="dup-group">
        <small class="mono">{{ g.sha256.slice(0, 16) }}… ・ {{ formatSize(g.size) }}</small>
        <div v-for="it in g.items" :key="it.summary.id" class="dup-item">
          <span class="mono">{{ it.file_path }}</span>
          <button class="danger small" @click="remove(it.summary.id)">ゴミ箱へ</button>
        </div>
      </div>
    </template>
  </div>

  <div class="card-box">
    <h4>削除の履歴（Windows のゴミ箱）</h4>
    <p v-if="!trash.length" class="hint">まだ削除はありません。</p>
    <div v-for="e in trash" :key="e.id" class="dup-item">
      <span>{{ e.model_name }} <small>{{ formatDate(e.at) }} ・ {{ e.paths.length }} ファイル</small></span>
      <button v-if="!e.restored" class="small" @click="restore(e)">元に戻す</button>
      <small v-else>戻し済み</small>
    </div>
  </div>
</template>
