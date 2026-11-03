<script setup lang="ts">
// Edit the fields LoRA Manager keeps in the sidecar.
import { reactive, ref } from 'vue'
import { ApiError, editModel, type ModelDetail } from '../api'
import { splitList } from '../actions'
import { toast } from '../toast'

const props = defineProps<{ d: ModelDetail }>()
const emit = defineEmits<{ (e: 'saved', d: ModelDetail): void; (e: 'close'): void }>()

const form = reactive({
  model_name: props.d.summary.name,
  base_model: props.d.summary.base_model,
  tags: props.d.tags.join(', '),
  trained_words: props.d.trained_words.join('\n'),
  notes: props.d.notes,
  strength: props.d.strength,
  favorite: props.d.summary.favorite,
  exclude: props.d.exclude,
})
const busy = ref(false)
const damaged = !!(props.d.damage || props.d.sidecar_error)
const repair = ref(false)

async function save() {
  busy.value = true
  try {
    const d = await editModel(props.d.summary.id, {
      model_name: form.model_name,
      base_model: form.base_model,
      tags: splitList(form.tags),
      trained_words: form.trained_words.split('\n').map((w) => w.trim()).filter(Boolean),
      notes: form.notes,
      strength: Number(form.strength),
      favorite: form.favorite,
      exclude: form.exclude,
      repair: repair.value,
    })
    toast('保存しました')
    emit('saved', d)
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
      <strong class="title">メタデータを編集</strong>
      <button class="close" aria-label="閉じる" @click="emit('close')">✕</button>
    </header>
    <div class="sheet-body">
      <p v-if="damaged" class="warn-box">
        このメタデータファイルは壊れています。保存すると読めた内容で書き直します（元のファイルは data/backups に残します）。
        <label class="check"><input v-model="repair" type="checkbox" /> 書き直して修復する</label>
      </p>
      <label class="field"><span>表示名</span><input v-model="form.model_name" type="text" /></label>
      <label class="field"><span>ベースモデル</span><input v-model="form.base_model" type="text" placeholder="Illustrious / Pony / SDXL 1.0 …" /></label>
      <label class="field"><span>タグ（カンマ区切り）</span><input v-model="form.tags" type="text" /></label>
      <label class="field"><span>トリガーワード（1 行に 1 つ）</span><textarea v-model="form.trained_words" rows="4"></textarea></label>
      <label class="field"><span>強さ（LoRA タグに使う値）</span><input v-model.number="form.strength" type="number" step="0.05" min="-10" max="10" /></label>
      <label class="field"><span>メモ</span><textarea v-model="form.notes" rows="3"></textarea></label>
      <label class="check"><input v-model="form.favorite" type="checkbox" /> お気に入り</label>
      <label class="check"><input v-model="form.exclude" type="checkbox" /> 一覧から隠す（除外）</label>
      <div class="actions">
        <button class="primary" :disabled="busy || (damaged && !repair)" @click="save">保存</button>
      </div>
    </div>
  </div>
</template>
