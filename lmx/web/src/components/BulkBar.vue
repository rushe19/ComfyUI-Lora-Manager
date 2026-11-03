<script setup lang="ts">
// Actions for the models selected in the library grid.
import { computed, ref } from 'vue'
import { ApiError, bulkAction, restoreTrash, startMetadata, type ModelType } from '../api'
import { splitList } from '../actions'
import { closeOverlay, isOpen, openOverlay } from '../overlay'
import { normalizeFolder, type Target } from '../targets'
import { toast } from '../toast'
import TargetPicker from './TargetPicker.vue'

const props = defineProps<{ ids: number[]; type: ModelType }>()
const emit = defineEmits<{ (e: 'done', clear: boolean): void; (e: 'select-all'): void; (e: 'clear'): void }>()

const busy = ref(false)
const input = ref('')
const inputKind = ref<'add_tags' | 'remove_tags' | 'base_model'>('add_tags')
const target = ref<Target>({ root: null, folder: '' })
const confirmDelete = ref(false)
const n = computed(() => props.ids.length)

async function run(action: string, extra: Record<string, unknown> = {}, clear = false) {
  if (!n.value) return
  busy.value = true
  try {
    const r = await bulkAction({ ids: props.ids, action, ...extra })
    const msg = r.errors.length ? `${r.ok} 件成功、${r.errors.length} 件失敗: ${r.errors[0]}` : `${r.ok} 件に適用しました`
    if (action === 'delete' && r.trash_ids.length) {
      const ids = r.trash_ids
      toast(`${r.ok} 件をゴミ箱へ移動しました`, 8000, {
        label: '元に戻す',
        run: async () => {
          let ok = 0
          for (const id of ids) {
            try {
              await restoreTrash(id)
              ok++
            } catch {
              /* reported below */
            }
          }
          toast(`${ok} / ${ids.length} 件を元に戻しました`, 3000)
          emit('done', false)
        },
      })
    } else {
      toast(msg, r.errors.length ? 5000 : 2000)
    }
    emit('done', clear)
  } catch (e) {
    toast(e instanceof ApiError ? e.message : String(e), 4000)
  } finally {
    busy.value = false
  }
}

function ask(kind: typeof inputKind.value) {
  inputKind.value = kind
  input.value = ''
  openOverlay('bulk-input')
}
async function submitInput() {
  const value = inputKind.value === 'base_model' ? input.value.trim() : splitList(input.value)
  closeOverlay()
  await run(inputKind.value, { value })
}
async function submitMove() {
  closeOverlay()
  await run('move', { root: target.value.root, folder: normalizeFolder(target.value.folder) }, true)
}
async function fetchMeta() {
  busy.value = true
  try {
    await startMetadata({ ids: props.ids, only_unlinked: false })
    toast('メタデータ取得を始めました（設定タブで進み具合を見られます）', 3000)
    emit('done', true)
  } catch (e) {
    toast(e instanceof ApiError ? e.message : String(e), 4000)
  } finally {
    busy.value = false
  }
}
const inputLabel = computed(() => ({ add_tags: '追加するタグ（カンマ区切り）', remove_tags: '外すタグ（カンマ区切り）', base_model: '新しいベースモデル' })[inputKind.value])
</script>

<template>
  <div class="bulk-bar">
    <div class="bulk-row">
      <strong>{{ n }} 件選択</strong>
      <button class="link" @click="emit('select-all')">読み込み済みを全選択</button>
      <button class="link" @click="emit('clear')">解除</button>
    </div>
    <div class="bulk-row scroll">
      <button :disabled="busy || !n" @click="run('favorite')">★ お気に入り</button>
      <button :disabled="busy || !n" @click="run('unfavorite')">☆ 外す</button>
      <button :disabled="busy || !n" @click="ask('add_tags')">＃ タグ追加</button>
      <button :disabled="busy || !n" @click="ask('remove_tags')">＃ タグ削除</button>
      <button :disabled="busy || !n" @click="ask('base_model')">ベース変更</button>
      <button :disabled="busy || !n" @click="openOverlay('bulk-move')">⇄ 移動</button>
      <button :disabled="busy || !n" @click="fetchMeta">⤓ メタデータ</button>
      <button class="danger" :disabled="busy || !n" @click="confirmDelete = true">🗑 削除</button>
    </div>
    <p v-if="confirmDelete" class="warn-box">
      {{ n }} 件と付属ファイルを Windows のゴミ箱へ移動します。
      <span class="actions"><button class="danger" @click="confirmDelete = false; run('delete', {}, true)">ゴミ箱へ移動</button><button @click="confirmDelete = false">やめる</button></span>
    </p>

    <div v-if="isOpen('bulk-input')" class="sheet-backdrop" @click.self="closeOverlay">
      <div class="sheet">
        <header class="sheet-head"><strong class="title">{{ inputLabel }}</strong><button class="close" aria-label="閉じる" @click="closeOverlay">✕</button></header>
        <div class="sheet-body">
          <label class="field"><span>{{ n }} 件に適用します</span><input v-model="input" type="text" enterkeyhint="done" @keyup.enter="submitInput" /></label>
          <div class="actions"><button class="primary" :disabled="!input.trim()" @click="submitInput">適用</button></div>
        </div>
      </div>
    </div>

    <div v-if="isOpen('bulk-move')" class="sheet-backdrop" @click.self="closeOverlay">
      <div class="sheet">
        <header class="sheet-head"><strong class="title">{{ n }} 件を移動</strong><button class="close" aria-label="閉じる" @click="closeOverlay">✕</button></header>
        <div class="sheet-body">
          <TargetPicker v-model="target" :type="type" />
          <div class="actions"><button class="primary" @click="submitMove">ここへ移動</button></div>
        </div>
      </div>
    </div>
  </div>
</template>
