import { api } from '../api/client'
import { type Run } from '../api/types'
import { t } from '../i18n'
import type { FileSource } from './links'

export type Preview = {
  path: string
  name: string
  runner_id: string
  format: 'text' | 'markdown' | 'json' | 'image'
  mime: string
  content: string
  size: number
  modified_at: string | null
  truncated: boolean
  image?: 'thumbnail' | 'original'
  revision?: string
  offset?: number
  next_offset?: number | null
}
export type ImageSource = FileSource & { document?: string; revision?: string }
export const imageMimes = ['image/png', 'image/jpeg', 'image/webp']
export async function executionId(source: FileSource, signal?: AbortSignal) {
  if (!source.nodeId) return source.runId
  const detail = await api<{ execution: Run | null }>(
    `/runs/${source.runId}/nodes/${encodeURIComponent(source.nodeId)}`,
    'GET',
    undefined,
    undefined,
    signal,
  )
  if (!detail.execution) throw new Error(t('filePreview.noSource'))
  return detail.execution.id
}
export function readPreview(
  id: string,
  href: string,
  signal?: AbortSignal,
  document?: string,
  image?: 'thumbnail' | 'original',
  offset = 0,
  revision?: string,
) {
  return api<Preview>(
    `/runs/${id}/files/preview`,
    'POST',
    {
      href,
      ...(document ? { document } : {}),
      ...(image ? { image } : {}),
      ...(offset ? { offset } : {}),
      ...(revision ? { revision } : {}),
    },
    undefined,
    signal,
  )
}

// Bound automatic image reads across all visible reports and preview modals.
let active = 0
const waiting: (() => void)[] = []
export function imageRead<T>(signal: AbortSignal, read: () => Promise<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    const abort = () => {
      const index = waiting.indexOf(start)
      if (index >= 0) waiting.splice(index, 1)
      reject(signal.reason ?? new DOMException('Aborted', 'AbortError'))
    }
    const start = () => {
      signal.removeEventListener('abort', abort)
      if (signal.aborted) {
        abort()
        return
      }
      active++
      read()
        .then(resolve, reject)
        .finally(() => {
          active--
          waiting.shift()?.()
        })
    }
    if (signal.aborted) {
      abort()
      return
    }
    if (active < 4) start()
    else {
      waiting.push(start)
      signal.addEventListener('abort', abort, { once: true })
    }
  })
}

// Originals are fetched only while a preview is open. Every chunk is independently
// authorized and revision-checked; never concatenate bytes from different files.
export async function readOriginal(
  id: string,
  href: string,
  signal: AbortSignal,
  document?: string,
) {
  let offset = 0
  let first: Preview | undefined
  const chunks: Uint8Array<ArrayBuffer>[] = []
  do {
    const chunk = await readPreview(id, href, signal, document, 'original', offset, first?.revision)
    if (
      chunk.format !== 'image' ||
      !imageMimes.includes(chunk.mime) ||
      chunk.truncated ||
      chunk.size > 32 * 1024 * 1024
    )
      throw new Error(t('filePreview.invalidOriginal'))
    // Legacy Runners can still return a complete small image; never loop on one.
    if (chunk.image !== 'original' && offset !== 0) throw new Error(t('filePreview.updateRunner'))
    const bytes = Uint8Array.from(atob(chunk.content), (c) => c.charCodeAt(0))
    if (chunk.image === 'original' && (chunk.offset !== offset || !chunk.revision))
      throw new Error(t('filePreview.invalidOriginal'))
    first ??= chunk
    const end = offset + bytes.length
    if (
      !bytes.length ||
      end > first.size ||
      chunk.size !== first.size ||
      chunk.path !== first.path ||
      chunk.mime !== first.mime ||
      chunk.revision !== first.revision
    )
      throw new Error(t('filePreview.invalidOriginal'))
    const next = chunk.next_offset
    if (next != null ? next !== end || next >= first.size : end !== first.size)
      throw new Error(t('filePreview.invalidOriginal'))
    chunks.push(bytes)
    offset = end
  } while (offset < first!.size)
  signal.throwIfAborted()
  return new Blob(chunks, { type: first!.mime })
}
