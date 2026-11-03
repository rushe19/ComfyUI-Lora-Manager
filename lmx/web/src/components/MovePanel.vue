<script setup lang="ts">
// Move to another folder and/or rename (companion files and the sidecar follow).
import { computed, ref } from 'vue'
import { ApiError, moveModel, type ModelDetail } from '../api'
import { normalizeFolder, type Target } from '../targets'
import { toast } from '../toast'
import TargetPicker from './TargetPicker.vue'

const props = defineProps<{ d: ModelDetail }>()
const emit = defineEmits<{ (e: 'moved', newId: number | null): void; (e: 'close'): void }>()

const initial: Target = { root: props.d.root, folder: props.d.summary.folder }
const target = ref<Target>({ ...initial })
const name = ref(props.d.summary.file_name)
const busy = ref(false)
const changed = computed(
  () => normalizeFolder(target.value.folder) !== props.d.summary.folder || target.value.root !== props.d.root || name.value.trim() !== props.d.summary.file_name,
)

async function run() {
  busy.value = true
  try {
    const r = await moveModel(props.d.summary.id, { root: target.value.root, folder: normalizeFolder(target.value.folder), name: name.value.trim() })
    toast('移動しました')
    emit('moved', r.id)
  } catch (e) {
    toast(e instanceof ApiError ? e.message : String(e), 4000)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="picker-panel">
    <header class="sheet-head">
      <strong class="title">移動・名前の変更</strong>
      <button class="close" aria-label="閉じる" @click="emit('close')">✕</button>
    </header>
    <div class="sheet-body">
      <label class="field"><span>ファイル名（拡張子なし）</span><input v-model="name" type="text" /></label>
      <p class="hint">プレビュー画像やメタデータなど、同じ名前の付属ファイルも一緒に移動・改名します。</p>
      <TargetPicker v-model="target" :type="d.summary.type" :base-model="d.summary.base_model" :initial="initial" />
      <div class="actions">
        <button class="primary" :disabled="busy || !changed" @click="run">移動する</button>
      </div>
    </div>
  </div>
</template>
