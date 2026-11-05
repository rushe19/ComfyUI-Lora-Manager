// Download file names: the user edits the part before the extension, like in the rename panel.

/** `a.v2.safetensors` -> `{ stem: 'a.v2', ext: '.safetensors' }` (no extension: `ext` is empty). */
export function splitFileName(name: string): { stem: string; ext: string } {
  const i = name.lastIndexOf('.')
  if (i <= 0) return { stem: name, ext: '' }
  return { stem: name.slice(0, i), ext: name.slice(i) }
}

/** The name the user typed, or `undefined` when it is empty or unchanged (the server then keeps Civitai's name). */
export function chosenName(stem: string, original: string): string | undefined {
  const { stem: origStem, ext } = splitFileName(original)
  const typed = stem.trim()
  if (!typed || typed === origStem) return undefined
  return typed + ext
}

const MAX_STEM = 150 // same cap as LoRA Manager's file name template

function clean(text: string): string {
  return text
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, '_')
    .replace(/_{2,}/g, '_')
    .replace(/^[-_. ]+|[-_. ]+$/g, '') // a dangling `_` from a replaced `?` must not double up with the joiner
}

/**
 * `<model name>_<version name>` as a file name stem, like LoRA Manager's `{model_name}_{version_name}`:
 * characters files cannot contain become `_`, the stem is capped (the model name is cut first, so the
 * version stays), and embeddings get `_` instead of spaces because their name is typed into prompts.
 */
export function autoStem(modelName: string, versionName: string, type?: string): string {
  const version = clean(versionName)
  let model = clean(modelName)
  const room = MAX_STEM - (version ? version.length + 1 : 0)
  if (model.length > room) model = model.slice(0, Math.max(room, 20))
  let stem = [model, version].filter(Boolean).join('_')
  if (type === 'embedding') stem = stem.replace(/\s+/g, '_')
  return stem.replace(/^[-_. ]+|[-_. ]+$/g, '').slice(0, MAX_STEM)
}
