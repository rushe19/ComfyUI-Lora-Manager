<script setup lang="ts">
// A muted looping preview video that only holds a decoder while it is on screen.
// Long Civitai result lists used to keep every video (even scrolled-off ones) loaded and playing,
// which made phones sluggish. Here the source is attached when the tile scrolls into view and
// released (src removed, buffers dropped) when it leaves, or while a sheet covers the list.
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { overlays } from '../overlay'

const props = withDefaults(defineProps<{ src: string; autoplay?: boolean; pauseBehindOverlay?: boolean; margin?: number }>(), {
  autoplay: true,
  pauseBehindOverlay: false,
  margin: 150,
})

const el = ref<HTMLVideoElement | null>(null)
const visible = ref(false)
let io: IntersectionObserver | null = null

function load() {
  const v = el.value
  if (!v) return
  if (v.getAttribute('src') !== props.src) v.src = props.src
  if (props.autoplay) void v.play().catch(() => {})
}
function unload() {
  const v = el.value
  if (!v || !v.hasAttribute('src')) return
  v.pause()
  v.removeAttribute('src')
  v.load() // drops the buffered data and the decoder
}
function sync() {
  const covered = props.pauseBehindOverlay && overlays.value.length > 0
  if (visible.value && !covered) load()
  else unload()
}

onMounted(() => {
  if (typeof IntersectionObserver === 'undefined') {
    visible.value = true
    sync()
    return
  }
  io = new IntersectionObserver(
    (entries) => {
      visible.value = entries[entries.length - 1].isIntersecting
      sync()
    },
    { rootMargin: `${props.margin}px 0px` },
  )
  if (el.value) io.observe(el.value)
})
onBeforeUnmount(() => {
  io?.disconnect()
  unload()
})
watch(() => [props.src, props.pauseBehindOverlay, overlays.value.length], sync)
</script>

<template>
  <video ref="el" muted playsinline loop preload="metadata" />
</template>
