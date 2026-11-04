<script setup lang="ts">
// Settings: library statistics (counts, sizes, breakdowns).
import { computed, onMounted, ref, watch } from 'vue'
import { fetchStats, type LibraryStats, type ModelType, type StatRow } from '../api'
import { formatSize } from '../util'

const type = ref<ModelType | ''>('')
const s = ref<LibraryStats | null>(null)
const error = ref('')
const by = ref<'count' | 'size'>('count')
const moreBase = ref(false)

async function load() {
  try {
    s.value = await fetchStats(type.value || undefined)
    error.value = ''
  } catch (e) {
    error.value = String(e)
  }
}
onMounted(load)
watch(type, load)

const pct = (n: number) => (s.value && s.value.total.count ? Math.round((n / s.value.total.count) * 100) : 0)
function bars(rows: StatRow[], key: 'count' | 'size') {
  const max = Math.max(1, ...rows.map((r) => r[key]))
  return rows.map((r) => ({ ...r, w: Math.max(1, Math.round((r[key] / max) * 100)) }))
}
const baseRows = computed(() => {
  if (!s.value) return []
  const rows = [...s.value.by_base].sort((a, b) => b[by.value] - a[by.value])
  return bars(moreBase.value ? rows : rows.slice(0, 12), by.value)
})
</script>

<template>
  <div class="card-box">
    <h4>統計</h4>
    <div class="chips">
      <select v-model="type" class="mini-select">
        <option value="">すべての種類</option>
        <option value="lora">LoRA</option>
        <option value="checkpoint">Checkpoint</option>
        <option value="embedding">Embedding</option>
        <option value="other">その他</option>
      </select>
      <button class="chip" @click="load">更新</button>
    </div>
    <p v-if="error" class="err-text">{{ error }}</p>
    <template v-if="s">
      <div class="stat-grid">
        <div><b>{{ s.total.count.toLocaleString() }}</b><small>モデル</small></div>
        <div><b>{{ formatSize(s.total.size) }}</b><small>合計サイズ</small></div>
        <div><b>{{ pct(s.linked) }}%</b><small>Civitai 情報あり（{{ s.linked.toLocaleString() }}）</small></div>
        <div><b>{{ pct(s.with_preview) }}%</b><small>プレビューあり</small></div>
        <div><b>{{ s.added_7d.count.toLocaleString() }}</b><small>7 日以内に追加（{{ formatSize(s.added_7d.size) }}）</small></div>
        <div><b>{{ s.added_30d.count.toLocaleString() }}</b><small>30 日以内に追加（{{ formatSize(s.added_30d.size) }}）</small></div>
        <div><b>{{ s.updates.toLocaleString() }}</b><small>新バージョンあり</small></div>
        <div><b>{{ s.favorites.toLocaleString() }}</b><small>お気に入り</small></div>
        <div><b>{{ s.duplicates.groups.toLocaleString() }}</b><small>重複の組（削れる容量 {{ formatSize(s.duplicates.reclaimable) }}）</small></div>
        <div><b>{{ s.damaged.toLocaleString() }}</b><small>要修復</small></div>
        <div><b>{{ s.civitai_models.toLocaleString() }}</b><small>Civitai のモデル数</small></div>
        <div v-if="s.excluded"><b>{{ s.excluded.toLocaleString() }}</b><small>除外中（集計外）</small></div>
      </div>

      <template v-if="!type">
        <h4>種類別</h4>
        <ul class="bars">
          <li v-for="r in bars(s.by_type, 'size')" :key="r.name"><span>{{ r.name }}</span><span class="bar" :style="{ width: r.w + '%' }"></span><small>{{ r.count.toLocaleString() }} 件 ・ {{ formatSize(r.size) }}</small></li>
        </ul>
      </template>

      <h4>
        ベースモデル別
        <span class="seg mini">
          <button :class="{ active: by === 'count' }" @click="by = 'count'">件数</button>
          <button :class="{ active: by === 'size' }" @click="by = 'size'">容量</button>
        </span>
      </h4>
      <ul class="bars">
        <li v-for="r in baseRows" :key="r.name"><span>{{ r.name }}</span><span class="bar" :style="{ width: r.w + '%' }"></span><small>{{ r.count.toLocaleString() }} 件 ・ {{ formatSize(r.size) }}</small></li>
      </ul>
      <button v-if="s.by_base.length > 12" class="link" @click="moreBase = !moreBase">{{ moreBase ? '閉じる' : `すべて表示（${s.by_base.length}）` }}</button>

      <h4>フォルダ別（容量順・最上位フォルダ）</h4>
      <ul class="bars">
        <li v-for="r in bars(s.by_folder, 'size')" :key="r.name"><span>{{ r.name }}</span><span class="bar" :style="{ width: r.w + '%' }"></span><small>{{ r.count.toLocaleString() }} 件 ・ {{ formatSize(r.size) }}</small></li>
      </ul>

      <template v-if="s.creators.length">
        <h4>作者別（件数順）</h4>
        <ul class="bars">
          <li v-for="r in bars(s.creators, 'count')" :key="r.name"><span>{{ r.name }}</span><span class="bar" :style="{ width: r.w + '%' }"></span><small>{{ r.count.toLocaleString() }} 件</small></li>
        </ul>
      </template>

      <h4>大きいファイル</h4>
      <ul class="bars">
        <li v-for="r in s.largest" :key="r.id"><span>{{ r.name }}</span><span></span><small>{{ formatSize(r.size) }}</small></li>
      </ul>
    </template>
  </div>
</template>
