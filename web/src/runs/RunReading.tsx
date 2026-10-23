import { ActionIcon } from '../actions/ActionIcon'
import { useCallback, useSyncExternalStore } from 'react'

import { InfoTrigger } from '../actions/InfoTrigger'
import { useTranslation } from 'react-i18next'
import { workspaceId } from '../api'
import { t } from '../i18n'
import { IconButton } from '../ui'
import './reading.css'
import { freshReading, type ReadingState } from './readingModel'
import { recordingReasons, type RecordingReason } from './recordingInfo'

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
export function TraceActions({
  id,
  issues,
}: {
  id: string
  issues?: ReadonlySet<RecordingReason>
}) {
  useTranslation()
  const [reading, update] = useRunReading(`execution:${id}`)
  const messages = recordingReasons
    .filter((reason) => issues?.has(reason))
    .map((reason) => t(`recording.${reason}`))
  return (
    <div className="action-group trace-actions" role="group" aria-label={t('actions.reading')}>
      {messages.length > 0 && (
        <InfoTrigger
          label={t('recording.info')}
          tooltip={messages.join(' ')}
          content={messages.map((message) => (
            <p key={message}>{message}</p>
          ))}
        />
      )}
      <IconButton
        label={t('reading.collapseAll')}
        onClick={() => update({ expanded: {}, autoExpand: false, follow: false })}
      >
        <ActionIcon name="collapseAll" />
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
        description={t(reading.follow ? 'actions.following' : 'actions.followPaused')}
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
        <ActionIcon name="latest" />
      </IconButton>
    </div>
  )
}
