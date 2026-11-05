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
