import { useCallback, useEffect, useRef, useState } from 'react'
import { useBlocker } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { t } from '../i18n'
import { Modal, Button } from '../components/ui'
import { DetailTabs } from '../components/DetailTabs'
import { TaskEditor, type TaskDefaults } from './TaskEditor'
import { DagEditor } from '../dag/DagEditor'
import { PresetCreation } from '../templates/PresetCreation'
import { useSearchControls } from '../search/context'
import '../templates/creation.css'

type CreationKind = 'task' | 'dag' | 'preset'
export function CreateTaskDialog({
  timezone,
  defaults,
  initialTab = 'task',
  presetId,
  onClose,
  onCreated,
}: {
  timezone: string
  defaults?: TaskDefaults
  initialTab?: CreationKind
  presetId?: string
  onClose: () => void
  onCreated?: (id: string, runId?: string | null) => void
}) {
  useTranslation()
  const [tab, setTab] = useState<CreationKind>(initialTab)
  const [visited, setVisited] = useState(() => new Set<CreationKind>([initialTab]))
  const [states, setStates] = useState<Record<string, { dirty: boolean; busy: boolean }>>({})
  const latest = useRef(states)
  latest.current = states
  const [confirm, setConfirm] = useState(false)
  const leaving = useRef(false)
  const leave = useRef<{ promise: Promise<boolean>; resolve: (value: boolean) => void } | null>(
    null,
  )
  const controls = useSearchControls()
  const update = useCallback(
    (kind: string, dirty: boolean, busy: boolean) =>
      setStates((previous) =>
        previous[kind]?.dirty === dirty && previous[kind]?.busy === busy
          ? previous
          : { ...previous, [kind]: { dirty, busy } },
      ),
    [],
  )
  const taskState = useCallback(
    (dirty: boolean, busy: boolean) => update('task', dirty, busy),
    [update],
  )
  const dagState = useCallback(
    (dirty: boolean, busy: boolean) => update('dag', dirty, busy),
    [update],
  )
  const presetState = useCallback(
    (dirty: boolean, busy: boolean) => update('preset', dirty, busy),
    [update],
  )
  const requestLeave = useCallback(() => {
    if (leaving.current) return Promise.resolve(true)
    if (Object.values(latest.current).some((value) => value.busy)) return Promise.resolve(false)
    if (!Object.values(latest.current).some((value) => value.dirty)) return Promise.resolve(true)
    if (leave.current) return leave.current.promise
    setConfirm(true)
    let resolve!: (value: boolean) => void
    const promise = new Promise<boolean>((done) => {
      resolve = done
    })
    leave.current = { promise, resolve }
    return promise
  }, [])
  const finishLeave = (allowed: boolean) => {
    if (allowed) leaving.current = true
    leave.current?.resolve(allowed)
    leave.current = null
    setConfirm(false)
  }
  useEffect(() => controls.registerGuard(requestLeave), [controls.registerGuard, requestLeave])
  useEffect(
    () => () => {
      leave.current?.resolve(false)
    },
    [],
  )
  useBlocker({
    shouldBlockFn: () => requestLeave().then((allowed) => !allowed),
    enableBeforeUnload: () =>
      !leaving.current && Object.values(latest.current).some((value) => value.dirty),
  })
  const close = async () => {
    if (await requestLeave()) onClose()
  }
  const created = (id: string, runId?: string | null) => {
    leaving.current = true
    onClose()
    onCreated?.(id, runId)
  }
  const busy = Object.values(states).some((value) => value.busy)
  return (
    <>
      <Modal
        open
        title={t('新建任务')}
        className="creation-modal"
        hideTitle
        actions={
          <DetailTabs
            idPrefix="creation"
            label={t('creation.kind')}
            value={tab}
            disabled={busy}
            tabs={[
              { key: 'task', label: t('任务') },
              { key: 'dag', label: 'DAG' },
              { key: 'preset', label: t('creation.preset') },
            ]}
            onChange={(next) => {
              if (!busy) {
                setVisited((previous) => new Set([...previous, next]))
                setTab(next)
              }
            }}
          />
        }
        onOpenChange={(open) => {
          if (!open) void close()
        }}
      >
        {(['task', 'dag', 'preset'] as const).map(
          (kind) =>
            visited.has(kind) && (
              <div
                key={kind}
                id={`creation-${kind}-panel`}
                role="tabpanel"
                aria-labelledby={`creation-${kind}-tab`}
                className="creation-panel"
                hidden={tab !== kind}
                inert={tab !== kind}
              >
                {kind === 'task' ? (
                  <TaskEditor
                    embedded
                    defaults={defaults}
                    defaultTimezone={timezone}
                    onClose={() => void close()}
                    onStateChange={taskState}
                    onCreated={created}
                  />
                ) : kind === 'dag' ? (
                  <DagEditor
                    embedded
                    active={tab === 'dag'}
                    timezone={timezone}
                    onClose={() => void close()}
                    onSaved={created}
                    onStateChange={dagState}
                  />
                ) : (
                  <PresetCreation
                    active={tab === 'preset'}
                    presetId={presetId}
                    onClose={() => void close()}
                    onCreated={created}
                    onStateChange={presetState}
                  />
                )}
              </div>
            ),
        )}
      </Modal>
      <Modal
        open={confirm}
        title={t('有未保存的更改')}
        onOpenChange={(open) => {
          if (!open) finishLeave(false)
        }}
      >
        <div className="modal-footer">
          <Button onClick={() => finishLeave(false)}>{t('继续编辑')}</Button>
          <Button variant="solid" onClick={() => finishLeave(true)}>
            {t('放弃更改')}
          </Button>
        </div>
      </Modal>
    </>
  )
}
