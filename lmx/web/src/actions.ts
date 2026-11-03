// Actions shared by the detail sheet and bulk selection.
import { ApiError, deleteModel, restoreTrash, type TrashEntry } from './api'
import { toast } from './toast'

/** Restore a Recycle Bin entry and report. Returns true on success. */
export async function undoTrash(entry: Pick<TrashEntry, 'id' | 'model_name'>, after?: () => void): Promise<boolean> {
  try {
    await restoreTrash(entry.id)
    toast(`「${entry.model_name}」を元に戻しました`)
    after?.()
    return true
  } catch (e) {
    toast(e instanceof ApiError ? e.message : String(e), 4000)
    return false
  }
}

/** Delete to the Recycle Bin with an undo toast. */
export async function trashWithUndo(id: number, after?: () => void): Promise<boolean> {
  try {
    const { trash } = await deleteModel(id)
    toast(`「${trash.model_name}」をゴミ箱へ移動しました`, 6000, { label: '元に戻す', run: () => void undoTrash(trash, after) })
    after?.()
    return true
  } catch (e) {
    toast(e instanceof ApiError ? e.message : String(e), 4000)
    return false
  }
}

/** Split a comma/newline separated list (tags, trigger words). */
export function splitList(text: string): string[] {
  return text
    .split(/[\n,、]/)
    .map((s) => s.trim())
    .filter(Boolean)
}
