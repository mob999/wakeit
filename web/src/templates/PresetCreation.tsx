import { useCallback, useEffect, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { api, ApiError } from '../api'
import { t } from '../i18n'
import { Button } from '../ui'
import { ErrorToast } from '../Toast'
import { Pagination } from '../Pagination'
import { TemplateInstanceView, useTemplateInstance } from './TemplateInstance'
import type { TemplateRecord } from './types'

export function PresetCreation({
  active,
  presetId,
  onClose,
  onCreated,
  onStateChange,
}: {
  active: boolean
  presetId?: string
  onClose: () => void
  onCreated: (id: string, runId?: string | null) => void
  onStateChange: (dirty: boolean, busy: boolean) => void
}) {
  useTranslation()
  const [selected, setSelected] = useState(presetId ?? '')
  const [query, setQuery] = useState('')
  const [page, setPage] = useState(1)
  const [choosing, setChoosing] = useState(!presetId)
  const [records, setRecords] = useState<Record<string, TemplateRecord>>({})
  const [states, setStates] = useState<Record<string, { dirty: boolean; busy: boolean }>>({})
  const list = useQuery<{ items: TemplateRecord[]; total: number }>({
    queryKey: ['task-templates', 'picker', query, page],
    queryFn: () =>
      api(
        `/task-templates?archived=false&q=${encodeURIComponent(query)}&page=${page}&page_size=20`,
      ),
    enabled: active && choosing,
  })
  const detail = useQuery<TemplateRecord>({
    queryKey: ['task-templates', 'detail', selected],
    queryFn: () => api(`/task-templates/${encodeURIComponent(selected)}`),
    enabled: active && !!selected && !choosing,
    refetchInterval: 10000,
    retry: false,
  })
  useEffect(() => {
    if (detail.data && detail.data.id === selected)
      setRecords((previous) =>
        previous[selected] ? previous : { ...previous, [selected]: detail.data! },
      )
  }, [selected, detail.data])
  const report = useCallback(
    (id: string, dirty: boolean, busy: boolean) =>
      setStates((previous) =>
        previous[id]?.dirty === dirty && previous[id]?.busy === busy
          ? previous
          : { ...previous, [id]: { dirty, busy } },
      ),
    [],
  )
  useEffect(() => {
    onStateChange(
      Object.values(states).some((v) => v.dirty),
      Object.values(states).some((v) => v.busy),
    )
  }, [states, onStateChange])
  const record = records[selected]
  const busy = Object.values(states).some((value) => value.busy)
  const latestRecord = detail.data?.id === selected ? detail.data : undefined
  const updated = !!record && !!latestRecord && latestRecord.revision !== record.revision
  const unavailable = latestRecord?.archived_at
    ? t('已归档')
    : detail.error instanceof ApiError && [403, 404].includes(detail.error.status)
      ? detail.error.message
      : ''
  return (
    <div className="preset-creation">
      <div className="preset-choice-heading">
        <strong>{record?.definition.name || t('creation.choosePreset')}</strong>
        {record && <span className="quiet">{record.definition.description}</span>}
        {updated && (
          <Button
            disabled={busy}
            onClick={() => setRecords((previous) => ({ ...previous, [selected]: latestRecord! }))}
          >
            {t('creation.loadRevision')}
          </Button>
        )}
        {record && (
          <Button
            disabled={busy}
            aria-expanded={choosing}
            onClick={() => setChoosing((value) => !value)}
          >
            {t(choosing ? 'creation.backToParameters' : 'creation.changePreset')}
          </Button>
        )}
      </div>
      {choosing && (
        <section className="preset-catalog" aria-label={t('creation.choosePreset')}>
          <input
            type="search"
            aria-label={t('creation.searchPresets')}
            placeholder={t('creation.searchPresets')}
            value={query}
            onChange={(event) => {
              setQuery(event.target.value)
              setPage(1)
            }}
          />
          <div className="preset-catalog-list">
            {list.isPending ? (
              <p className="quiet padded" role="status">
                {t('正在加载…')}
              </p>
            ) : list.error ? (
              <ErrorToast>
                {list.error.message}
                <Button onClick={() => list.refetch()}>{t('重试')}</Button>
              </ErrorToast>
            ) : !list.data?.items.length ? (
              <p className="quiet padded">{t(query ? '无匹配结果' : 'preset.empty')}</p>
            ) : (
              list.data.items.map((row) => (
                <button
                  type="button"
                  key={row.id}
                  className={`preset-choice ${selected === row.id ? 'active' : ''}`}
                  aria-pressed={selected === row.id}
                  disabled={busy}
                  onClick={() => {
                    setRecords((previous) =>
                      previous[row.id] ? previous : { ...previous, [row.id]: row },
                    )
                    setSelected(row.id)
                    setChoosing(false)
                  }}
                >
                  <strong>{row.definition.name}</strong>
                  <small>
                    {row.definition.task.dag ? 'DAG' : t('任务')} ·{' '}
                    {t('taskOverview.parameterCount', {
                      count: Object.keys(row.definition.inputs.properties ?? {}).length,
                    })}
                  </small>
                  {row.definition.description && <span>{row.definition.description}</span>}
                </button>
              ))
            )}
          </div>
          <Pagination
            page={page}
            total={list.data?.total ?? 0}
            pending={list.isFetching}
            onChange={setPage}
          />
        </section>
      )}
      {selected && !record && (
        <div className="preset-detail-state">
          {detail.error ? (
            <ErrorToast>
              {detail.error.message}
              <Button onClick={() => detail.refetch()}>{t('重试')}</Button>
            </ErrorToast>
          ) : (
            <p role="status">{t('正在加载…')}</p>
          )}
        </div>
      )}
      {Object.values(records).map((item) => (
        <PresetSession
          key={item.id}
          record={item}
          unavailable={item.id === selected ? unavailable : ''}
          updated={item.id === selected && updated}
          active={active && !choosing && item.id === selected}
          hidden={choosing || item.id !== selected}
          onStateChange={report}
          onClose={onClose}
          onCreated={onCreated}
        />
      ))}
      {(!selected || choosing || !record) && (
        <div className="modal-footer">
          {record && choosing ? (
            <Button onClick={() => setChoosing(false)}>{t('creation.backToParameters')}</Button>
          ) : (
            <Button onClick={onClose}>{t('取消')}</Button>
          )}
        </div>
      )}
    </div>
  )
}
function PresetSession({
  record,
  active,
  hidden,
  onStateChange,
  onClose,
  onCreated,
  unavailable,
  updated,
}: {
  unavailable: string
  updated: boolean
  record: TemplateRecord
  active: boolean
  hidden: boolean
  onStateChange: (id: string, dirty: boolean, busy: boolean) => void
  onClose: () => void
  onCreated: (id: string, runId?: string | null) => void
}) {
  const instance = useTemplateInstance({ record, active })
  const [error, setError] = useState('')
  const [issueRequest, setIssueRequest] = useState(0)
  useEffect(() => setError(''), [record.revision])
  const submitting = useRef(false)
  useEffect(() => {
    onStateChange(record.id, instance.dirty, instance.pending)
  }, [record.id, instance.dirty, instance.pending, onStateChange])
  const submit = async (run: boolean) => {
    if (submitting.current) return
    submitting.current = true
    setError('')
    try {
      const result = await instance.create(run)
      onCreated(result.task_id, result.run_id)
    } catch (failure) {
      setError((failure as Error).message)
    } finally {
      submitting.current = false
    }
  }
  const blocked =
    unavailable ||
    updated ||
    record.archived_at ||
    !instance.current ||
    !instance.preview?.config ||
    instance.pendingPreview
  const count = instance.issues.length
  return (
    <div className="preset-session" hidden={hidden} inert={hidden}>
      <fieldset disabled={instance.pending} className="preset-session-fields">
        <TemplateInstanceView instance={instance} issueRequest={issueRequest} />
      </fieldset>
      <div className="modal-footer">
        <div className="creation-validation" role="status">
          {error ? (
            <ErrorToast>{error}</ErrorToast>
          ) : unavailable ? (
            unavailable
          ) : updated ? (
            t('creation.presetUpdated')
          ) : record.archived_at ? (
            t('已归档')
          ) : instance.pendingPreview ? (
            t('taskTemplate.updating')
          ) : count ? (
            <Button onClick={() => setIssueRequest((value) => value + 1)}>
              {t('creation.viewIssues', { count })}
            </Button>
          ) : instance.current ? (
            t('creation.ready')
          ) : (
            t('taskTemplate.fillInputs')
          )}
        </div>
        <Button disabled={instance.pending} onClick={onClose}>
          {t('取消')}
        </Button>
        <Button
          disabled={!!blocked || instance.pending}
          loading={instance.pending}
          onClick={() => void submit(false)}
        >
          {t('creation.create')}
        </Button>
        <Button
          variant="solid"
          disabled={!!blocked || instance.pending}
          onClick={() => void submit(true)}
        >
          {t('creation.createRun')}
        </Button>
      </div>
    </div>
  )
}
