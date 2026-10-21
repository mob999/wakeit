import { useCallback, useSyncExternalStore } from 'react'
import { ArrowDown, ChevronsUp } from 'lucide-react'
import { workspaceId } from '../api'
import { t } from '../i18n'
import { IconButton } from '../ui'
import './reading.css'
import { freshReading, type ReadingState } from './readingModel'

type Store = { value: ReadingState; listeners: Set<() => void> }
const stores = new Map<string, Store>()
let currentWorkspace = workspaceId
function storeFor(id: string) {
  if (currentWorkspace !== workspaceId) {
    stores.clear()
    currentWorkspace = workspaceId
  }
  const key = `${workspaceId ?? 'account'}:${id}`
  if (!stores.has(key)) stores.set(key, { value: freshReading(), listeners: new Set() })
  return stores.get(key)!
}
export function useRunReading(id: string) {
  const store = storeFor(id)
  const subscribe = useCallback(
    (listener: () => void) => {
      store.listeners.add(listener)
      return () => {
        store.listeners.delete(listener)
      }
    },
    [store],
  )
  const snapshot = useCallback(() => store.value, [store])
  const state = useSyncExternalStore(subscribe, snapshot)
  const update = useCallback(
    (change: Partial<ReadingState> | ((old: ReadingState) => ReadingState)) => {
      const next =
        typeof change === 'function' ? change(store.value) : { ...store.value, ...change }
      if (next === store.value) return
      store.value = next
      store.listeners.forEach((fn) => fn())
    },
    [store],
  )
  return [state, update] as const
}
export function TraceActions({ id }: { id: string }) {
  const [reading, update] = useRunReading(`execution:${id}`)
  return (
    <>
      <IconButton
        label={t('reading.collapseAll')}
        onClick={() => update({ expanded: {}, autoExpand: false, follow: false })}
      >
        <ChevronsUp />
      </IconButton>
      {reading.unseen.length > 0 && (
        <button
          type="button"
          className="trace-new-updates"
          onClick={() =>
            update((old) => ({
              ...old,
              follow: true,
              autoExpand: true,
              unseen: [],
              jump: old.jump + 1,
            }))
          }
        >
          {t('reading.updates', { count: reading.unseen.length })}
        </button>
      )}
      <IconButton
        label={t('reading.latest')}
        onClick={() =>
          update((old) => ({
            ...old,
            follow: true,
            autoExpand: true,
            unseen: [],
            jump: old.jump + 1,
          }))
        }
      >
        <ArrowDown />
      </IconButton>
    </>
  )
}
