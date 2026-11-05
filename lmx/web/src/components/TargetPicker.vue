<script setup lang="ts">
// Destination picker: root (when several) + existing folder or a new one.
import { computed, onMounted, ref, watch } from 'vue'
import { downloadTargets, type ModelType, type Targets } from '../api'
import { lastTarget, normalizeFolder, suggestFolders, type Target } from '../targets'

const props = defineProps<{ type: ModelType; baseModel?: string; initial?: Target }>()
const model = defineModel<Target>({ required: true })

const targets = ref<Targets | null>(null)
const filter = ref('')
const error = ref('')

async function load() {
  try {
    targets.value = await downloadTargets(props.type)
    const last = props.initial ?? lastTarget(props.type)
    model.value = { root: last.root ?? targets.value.roots[0] ?? null, folder: last.folder }
  } catch (e) {
    error.value = String(e)
  }
}
onMounted(load)
watch(() => props.type, load)

const shown = computed(() => {
  const n = filter.value.trim().toLowerCase()
  const list = targets.value?.folders ?? []
  return (n ? list.filter((f) => f.toLowerCase().includes(n)) : list).slice(0, 200)
})
const fullPath = computed(() => [model.value.root, normalizeFolder(model.value.folder)].filter(Boolean).join('/'))
// Existing top-level folders that look like this model's base model (e.g. "Krea 2" -> "Krea2").
const suggested = computed(() => suggestFolders(targets.value?.folders ?? [], props.baseModel ?? ''))
</script>

<template>
  <div class="target-picker">
    <p v-if="error" class="err">{{ error }}</p>
    <template v-if="targets">
      <p v-if="!targets.roots.length" class="err">この種類の保存先（ルートフォルダ）が設定されていません。</p>
      <label v-if="targets.roots.length > 1" class="field">
        <span>ルート</span>
        <select v-model="model.root">
          <option v-for="r in targets.roots" :key="r" :value="r">{{ r }}</option>
        </select>
      </label>
      <label class="field">
        <span>フォルダ（新しい名前も入力できます）</span>
        <input v-model="model.folder" type="text" placeholder="例: Character/Name（空ならルート直下）" />
      </label>
      <div v-if="suggested.length" class="chips">
        <span class="hint">おすすめ:</span>
        <button v-for="s in suggested" :key="s" class="chip" :class="{ active: normalizeFolder(model.folder) === s }" @click="model.folder = s">{{ s }}</button>
      </div>
      <input v-model="filter" type="search" placeholder="既存のフォルダを検索" />
      <ul class="folder-list compact">
        <li :class="{ active: normalizeFolder(model.folder) === '' }" @click="model.folder = ''">（ルート直下）</li>
        <li v-for="f in shown" :key="f" :class="{ active: normalizeFolder(model.folder) === f }" @click="model.folder = f">{{ f }}</li>
      </ul>
      <p class="hint mono">保存先: {{ fullPath }}</p>
    </template>
  </div>
</template>
