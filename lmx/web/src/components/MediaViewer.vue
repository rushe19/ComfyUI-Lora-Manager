<script setup lang="ts">
// Fullscreen viewer for sample images / videos: swipe or arrows to move, tap outside or ✕ to close.
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { prefs } from '../prefs'
import { isMature } from '../util'

export interface ViewerItem {
  url: string
  video: boolean
  nsfw: number
}

const props = defineProps<{ items: ViewerItem[]; start: number }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const index = ref(Math.min(Math.max(props.start, 0), props.items.length - 1))
const item = computed(() => props.items[index.value])
const revealed = ref(false)
const failed = ref(false)
const blurred = computed(() => prefs.blurMature && isMature(item.value?.nsfw ?? 0) && !revealed.value)

function go(d: number) {
  const n = index.value + d
  if (n < 0 || n >= props.items.length) return
  index.value = n
  revealed.value = false
  failed.value = false
}

// Horizontal swipe.
let x0 = 0
let y0 = 0
function down(e: PointerEvent) {
  x0 = e.clientX
  y0 = e.clientY
}
function up(e: PointerEvent) {
  const dx = e.clientX - x0
  const dy = e.clientY - y0
  if (Math.abs(dx) > 50 && Math.abs(dx) > Math.abs(dy) * 1.5) go(dx < 0 ? 1 : -1)
}
function key(e: KeyboardEvent) {
  if (e.key === 'Escape') emit('close')
  else if (e.key === 'ArrowLeft') go(-1)
  else if (e.key === 'ArrowRight') go(1)
}
onMounted(() => window.addEventListener('keydown', key))
onBeforeUnmount(() => window.removeEventListener('keydown', key))
</script>

<template>
  <div class="viewer" @click.self="emit('close')" @pointerdown="down" @pointerup="up">
    <button class="viewer-close" aria-label="閉じる" @click="emit('close')">✕</button>
    <span v-if="items.length > 1" class="viewer-count">{{ index + 1 }} / {{ items.length }}</span>
    <button v-if="index > 0" class="viewer-nav prev" aria-label="前へ" @click.stop="go(-1)">‹</button>
    <button v-if="index < items.length - 1" class="viewer-nav next" aria-label="次へ" @click.stop="go(1)">›</button>
    <video v-if="item.video" :key="item.url" :src="item.url" controls autoplay loop playsinline :class="{ blur: blurred }" @click="revealed = true" @error="failed = true" />
    <img v-else :key="item.url" :src="item.url" alt="" :class="{ blur: blurred }" @click="revealed = true" @error="failed = true" />
    <p v-if="blurred" class="viewer-hint">タップでぼかしを外す</p>
    <p v-if="failed" class="viewer-hint err">読み込めませんでした</p>
  </div>
</template>
