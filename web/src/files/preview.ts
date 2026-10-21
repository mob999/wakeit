import { api, type Run } from '../api'
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
export function readPreview(id: string, href: string, signal?: AbortSignal, document?: string) {
  return api<Preview>(
    `/runs/${id}/files/preview`,
    'POST',
    { href, ...(document ? { document } : {}) },
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
