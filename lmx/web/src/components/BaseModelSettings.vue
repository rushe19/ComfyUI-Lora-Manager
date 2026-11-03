<script setup lang="ts">
// Settings: which base models appear as filter chips in Civitai search (stored on the server).
import { computed, onMounted, ref } from 'vue'
import { baseModels, loadBaseModels, setSelected, toggleSelected } from '../baseModels'

const filter = ref('')
const onlySelected = ref(false)
onMounted(() => void loadBaseModels(true))

const shown = computed(() => {
  const n = filter.value.trim().toLowerCase()
  return baseModels.all.filter((b) => (!n || b.toLowerCase().includes(n)) && (!onlySelected.value || baseModels.selected.includes(b)))
})
function selectLocal() {
  setSelected([...new Set([...baseModels.selected, ...Object.keys(baseModels.local).filter((n) => baseModels.all.includes(n))])])
}
</script>

<template>
  <div class="card-box">
    <h4>Civitai 検索のベースモデル</h4>
    <p class="hint">選んだものが検索画面の絞り込みに並びます。手元に無い種類（MOSS Image など）も選べます。設定はサーバーに保存され、スマホと PC で共通です。</p>
    <p v-if="baseModels.error" class="err-text">{{ baseModels.error }}</p>
    <div class="search-row">
      <input v-model="filter" type="search" placeholder="名前で絞り込み" />
    </div>
    <div class="chips">
      <label class="chip"><input v-model="onlySelected" type="checkbox" /> 選択中のみ</label>
      <button class="chip" @click="selectLocal">手元にあるものを追加</button>
      <button class="chip" @click="setSelected([])">すべて解除</button>
      <span class="hint">{{ baseModels.selected.length }} / {{ baseModels.all.length }} 件選択<template v-if="baseModels.saving"> ・ 保存中…</template></span>
    </div>
    <ul class="bm-list">
      <li v-for="b in shown" :key="b">
        <label class="check">
          <input type="checkbox" :checked="baseModels.selected.includes(b)" @change="toggleSelected(b)" />
          {{ b }}
          <span v-if="baseModels.fresh.includes(b)" class="badge new">新着</span>
          <span v-if="baseModels.local[b]" class="badge">手元 {{ baseModels.local[b] }}</span>
        </label>
      </li>
    </ul>
  </div>
</template>
