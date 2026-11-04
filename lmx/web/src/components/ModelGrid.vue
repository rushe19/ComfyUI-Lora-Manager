<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import type { ModelSummary } from '../api'
import { prefs } from '../prefs'
import LazyVideo from './LazyVideo.vue'
import { computeGrid, isMature, visibleRows } from '../util'

const props = defineProps<{ items: ModelSummary[]; total: number; loading: boolean; selected?: Set<number> }>()
const emit = defineEmits<{ (e: 'need-more'): void; (e: 'open', id: number): void; (e: 'long', id: number): void }>()

// Long press (500 ms without moving) selects instead of opening.
let pressTimer: number | undefined
let pressAt: { x: number; y: number } | null = null
let longFired = false
function onDown(id: number, ev: PointerEvent) {
  longFired = false
  pressAt = { x: ev.clientX, y: ev.clientY }
  window.clearTimeout(pressTimer)
  pressTimer = window.setTimeout(() => {
    longFired = true
    navigator.vibrate?.(15)
    emit('long', id)
  }, 500)
}
function onMove(ev: PointerEvent) {
  if (pressAt && Math.hypot(ev.clientX - pressAt.x, ev.clientY - pressAt.y) > 10) window.clearTimeout(pressTimer)
}
function onUp() {
  window.clearTimeout(pressTimer)
  pressAt = null
}
function onClick(id: number) {
  if (longFired) {
    longFired = false
    return
  }
  emit('open', id)
}

const scroller = ref<HTMLElement | null>(null)
const width = ref(360)
const viewport = ref(640)
const scrollTop = ref(0)
const GAP = 8
const CAPTION = 50

const layout = computed(() => computeGrid(width.value, width.value < 600 ? 150 : 190, GAP, CAPTION))
const rows = computed(() => Math.ceil(props.items.length / layout.value.cols))
const range = computed(() => visibleRows(scrollTop.value, viewport.value, layout.value.rowHeight, rows.value))
const visible = computed(() => {
  const [a, b] = range.value
  const { cols } = layout.value
  return props.items.slice(a * cols, b * cols).map((item, i) => ({ item, index: a * cols + i }))
})

function onScroll() {
  if (!scroller.value) return
  scrollTop.value = scroller.value.scrollTop
  maybeLoad()
}
function maybeLoad() {
  // Ask for the next page when within ~3 screens of the end of what is loaded.
  const loadedHeight = rows.value * layout.value.rowHeight
  if (!props.loading && props.items.length < props.total && scrollTop.value + viewport.value * 3 > loadedHeight) emit('need-more')
}
const ro = new ResizeObserver(() => {
  if (!scroller.value) return
  width.value = scroller.value.clientWidth
  viewport.value = scroller.value.clientHeight
  maybeLoad()
})
onMounted(() => scroller.value && ro.observe(scroller.value))
onBeforeUnmount(() => ro.disconnect())
watch(
  () => props.items.length,
  (n, old) => {
    if (n === 0 && old > 0 && scroller.value) scroller.value.scrollTop = 0
    requestAnimationFrame(maybeLoad)
  },
)

function style(index: number) {
  const { cols, cardWidth, rowHeight } = layout.value
  const r = Math.floor(index / cols)
  const c = index % cols
  return { transform: `translate(${GAP + c * (cardWidth + GAP)}px, ${r * rowHeight}px)`, width: `${cardWidth}px` }
}
</script>

<template>
  <div ref="scroller" class="grid-scroller" @scroll.passive="onScroll">
    <div class="grid-inner" :style="{ height: rows * layout.rowHeight + 'px' }">
      <button
        v-for="{ item, index } in visible"
        :key="item.id"
        class="card"
        :class="{ selected: selected?.has(item.id) }"
        :style="style(index)"
        @pointerdown="onDown(item.id, $event)"
        @pointermove="onMove"
        @pointerup="onUp"
        @pointercancel="onUp"
        @pointerleave="onUp"
        @contextmenu.prevent
        @click="onClick(item.id)"
      >
        <div class="thumb" :class="{ blur: prefs.blurMature && isMature(item.nsfw_level) }">
          <LazyVideo v-if="item.preview && item.preview_kind === 'video' && prefs.showVideos" :src="item.preview" pause-behind-overlay />
          <img v-else-if="item.preview && item.preview_kind === 'image'" :src="item.preview" decoding="async" alt="" />
          <div v-else class="noimg">No preview</div>
          <span class="base">{{ item.base_model }}</span>
          <span v-if="item.favorite" class="fav">★</span>
          <span v-if="item.damaged" class="damaged">要修復</span>
          <span v-else-if="item.update" class="update-badge" title="Civitai に新しいバージョンがあります">NEW</span>
          <span v-else-if="!item.linked" class="unlinked" title="メタデータ未取得">?</span>
          <span v-if="selected" class="check">{{ selected.has(item.id) ? '✓' : '' }}</span>
        </div>
        <div class="caption">{{ item.name }}</div>
      </button>
    </div>
    <p v-if="loading" class="loading">読み込み中…</p>
    <p v-else-if="!items.length" class="empty">該当するモデルがありません</p>
  </div>
</template>
