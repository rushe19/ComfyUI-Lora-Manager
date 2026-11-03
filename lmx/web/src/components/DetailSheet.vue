<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { fetchDetail, type ModelDetail } from '../api'
import { prefs } from '../prefs'
import { toast } from '../toast'
import { copyText, formatDate, formatSize, htmlToText, isMature } from '../util'

const props = defineProps<{ id: number }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const d = ref<ModelDetail | null>(null)
const error = ref('')
const showDesc = ref(false)
const description = computed(() => (d.value ? htmlToText(d.value.model_description || d.value.version_description) : ''))
const preview = computed(() => d.value?.summary.preview ?? null)

onMounted(async () => {
  try {
    d.value = await fetchDetail(props.id)
  } catch (e) {
    error.value = String(e)
  }
})

async function copy(text: string, label: string) {
  toast((await copyText(text)) ? `${label}をコピーしました` : 'コピーできませんでした')
}
</script>

<template>
  <div class="sheet-backdrop" @click.self="emit('close')">
    <article class="sheet detail">
      <header class="sheet-head">
        <strong class="title">{{ d?.summary.name ?? '読み込み中…' }}</strong>
        <button class="close" aria-label="閉じる" @click="emit('close')">✕</button>
      </header>
      <div class="sheet-body">
        <p v-if="error" class="err">{{ error }}</p>
        <template v-if="d">
          <div class="hero" :class="{ blur: prefs.blurMature && isMature(d.summary.nsfw_level) }">
            <video v-if="preview && d.summary.preview_kind === 'video'" :src="preview" controls muted playsinline loop autoplay />
            <img v-else-if="preview" :src="preview" alt="" />
          </div>

          <p v-if="d.damage || d.sidecar_error" class="warn-box">
            メタデータ（.metadata.json）が壊れています。{{ d.damage ? '読める部分だけ表示しています。' : d.sidecar_error }}
          </p>

          <div class="meta-chips">
            <span class="chip">{{ d.summary.base_model }}</span>
            <span v-if="d.version_name" class="chip">{{ d.version_name }}</span>
            <span class="chip">{{ formatSize(d.summary.size) }}</span>
            <span class="chip">{{ formatDate(d.summary.modified) }}</span>
            <span v-if="d.creator" class="chip">by {{ d.creator }}</span>
          </div>

          <div class="actions">
            <button v-if="d.lora_syntax" class="primary" @click="copy(d.lora_syntax, 'LoRA タグ')">{{ d.lora_syntax }} をコピー</button>
            <button v-if="d.trained_words.length" @click="copy(d.trained_words.join(', '), 'トリガーワード')">トリガーワードを全部コピー</button>
            <a v-if="d.civitai_url" class="button" :href="d.civitai_url" target="_blank" rel="noopener noreferrer">Civitai で開く ↗</a>
          </div>

          <section v-if="d.trained_words.length">
            <h4>トリガーワード <small>（タップでコピー）</small></h4>
            <div class="chips">
              <button v-for="w in d.trained_words" :key="w" class="chip word" @click="copy(w, `「${w}」`)">{{ w }}</button>
            </div>
          </section>

          <section v-if="d.tags.length">
            <h4>タグ</h4>
            <div class="chips"><span v-for="t in d.tags" :key="t" class="chip">{{ t }}</span></div>
          </section>

          <section v-if="d.notes">
            <h4>メモ</h4>
            <p class="pre">{{ d.notes }}</p>
          </section>

          <section v-if="description">
            <h4>説明</h4>
            <p class="pre" :class="{ clamp: !showDesc }">{{ description }}</p>
            <button class="link" @click="showDesc = !showDesc">{{ showDesc ? '閉じる' : 'もっと見る' }}</button>
          </section>

          <section v-if="d.images.length">
            <h4>Civitai のサンプル画像</h4>
            <div class="strip">
              <template v-for="img in d.images" :key="img.url">
                <video v-if="img.type === 'video'" :src="img.url" muted playsinline loop preload="none" controls :class="{ blur: prefs.blurMature && isMature(img.nsfw_level ?? 0) }" />
                <img v-else :src="img.url" loading="lazy" alt="" :class="{ blur: prefs.blurMature && isMature(img.nsfw_level ?? 0) }" />
              </template>
            </div>
          </section>

          <section>
            <h4>ファイル</h4>
            <dl class="kv">
              <dt>場所</dt>
              <dd class="mono" @click="copy(d.file_path, 'パス')">{{ d.file_path }}</dd>
              <dt>SHA256</dt>
              <dd class="mono" @click="d.sha256 && copy(d.sha256, 'ハッシュ')">{{ d.sha256 ?? '未計算' }}</dd>
              <dt>取得元</dt>
              <dd>{{ d.metadata_source ?? '未取得' }}</dd>
            </dl>
          </section>
        </template>
      </div>
    </article>
  </div>
</template>
