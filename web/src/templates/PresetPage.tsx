import YAML from 'yaml'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useBlocker, useNavigate, useSearch } from '@tanstack/react-router'
import * as Dropdown from '@radix-ui/react-dropdown-menu'
import { api, time, type User } from '../api'
import { t } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useSearchControls } from '../Search'
import { Split, DetailBody } from '../Workbench'
import { DetailTabs } from '../DetailTabs'
import { PageActions } from '../PageHeader'
import { ActionMenu, ActionMenuItem } from '../actions/ActionMenu'
import { ActionIcon } from '../actions/ActionIcon'
import { Button, IconButton, Modal } from '../ui'
import { ContentCard } from '../ContentCard'
import { ErrorToast, useToast } from '../Toast'
import { Pagination } from '../Pagination'
import { ChevronRight } from '../icons'
import { AuthorEditor, starter } from './AuthorEditor'
import { TemplateInstanceView, useTemplateInstance } from './TemplateInstance'
import { definitionFrom } from './utils'
import type { Definition, TemplateRecord } from './types'
import './templates.css'

type Section = 'overview' | 'preview' | 'definition'
type NewPreset = 'task' | 'dag'

export function PresetPage({ user }: { user: User }) {
  useTranslation()
  const navigate = useNavigate()
  const search = useSearch({ from: '/presets' })
  const controls = useSearchControls()
  const client = useQueryClient()
  const toast = useToast()
  const editable = user.role !== 'viewer' && !user.workspace?.is_demo
  const [query, setQuery] = useState('')
  const [archived, setArchived] = useState(false)
  const [page, setPage] = useState(1)
  const list = useQuery<{ items: TemplateRecord[]; total: number }>({
    queryKey: ['task-templates', 'list', query, archived, page],
    queryFn: () =>
      api(`/task-templates?q=${encodeURIComponent(query)}&archived=${archived}&page=${page}`),
    refetchInterval: 10000,
  })
  const selected = useQuery<TemplateRecord>({
    queryKey: ['task-templates', 'detail', search.template],
    queryFn: () => api(`/task-templates/${encodeURIComponent(search.template!)}`),
    enabled: !!search.template && !search.new,
  })
  const record = search.new
    ? null
    : (selected.data ??
      list.data?.items.find((row) => row.id === search.template) ??
      list.data?.items[0] ??
      null)
  const [source, setSource] = useState('')
  const [baseline, setBaseline] = useState('')
  const dirty = source !== baseline
  const dirtyRef = useRef(dirty)
  dirtyRef.current = dirty
  const [leaveOpen, setLeaveOpen] = useState(false)
  const leavePromise = useRef<Promise<boolean> | null>(null)
  const leaveResolve = useRef<((allowed: boolean) => void) | null>(null)
  const requestLeave = useCallback(() => {
    if (!dirtyRef.current) return Promise.resolve(true)
    if (leavePromise.current) return leavePromise.current
    setLeaveOpen(true)
    leavePromise.current = new Promise<boolean>((resolve) => {
      leaveResolve.current = resolve
    })
    return leavePromise.current
  }, [])
  const finishLeave = (allowed: boolean) => {
    if (allowed) {
      setSource(baseline)
      dirtyRef.current = false
    }
    leaveResolve.current?.(allowed)
    leaveResolve.current = null
    leavePromise.current = null
    setLeaveOpen(false)
  }
  useEffect(() => controls.registerGuard(requestLeave), [controls.registerGuard, requestLeave])
  useEffect(
    () => () => {
      leaveResolve.current?.(false)
    },
    [],
  )
  useBlocker({
    shouldBlockFn: () => requestLeave().then((allowed) => !allowed),
    enableBeforeUnload: () => dirtyRef.current,
  })

  useEffect(() => {
    if (search.new) return
    if (!record || dirtyRef.current) return
    const next = YAML.stringify(record.definition, { lineWidth: 0 })
    setSource(next)
    setBaseline(next)
  }, [record?.id, record?.revision, search.new])
  useEffect(() => {
    if (!search.new) return
    let live = true
    if (search.source) {
      api<Definition>(`/tasks/${encodeURIComponent(search.source)}/template`)
        .then((definition) => {
          if (!live) return
          const next = YAML.stringify(definition, { lineWidth: 0 })
          setSource(next)
          setBaseline(next)
        })
        .catch((error) => toast(error.message))
    } else {
      const next = YAML.stringify(starter(search.new === 'dag'), { lineWidth: 0 })
      setSource(next)
      setBaseline(next)
    }
    return () => {
      live = false
    }
  }, [search.new, search.source])

  let definition: Definition | null = null,
    definitionError = ''
  try {
    if (source) definition = definitionFrom(source)
    else if (record) definition = record.definition
  } catch (error) {
    definitionError = (error as Error).message
  }
  const section: Section = search.section ?? 'overview'
  const instance = useTemplateInstance({
    record,
    definition,
    definitionError,
    taskId: record?.id ?? 'template-preview',
  })
  const save = useMutation({
    mutationFn: (next: Definition) =>
      api<TemplateRecord>(`/task-templates/${encodeURIComponent(next.id)}`, 'PUT', {
        definition: next,
        expected_revision: record?.revision ?? null,
      }),
    onSuccess: (next) => {
      const value = YAML.stringify(next.definition, { lineWidth: 0 })
      setSource(value)
      setBaseline(value)
      dirtyRef.current = false
      client.invalidateQueries({ queryKey: ['task-templates'] })
      toast(t('taskTemplate.saved'))
      navigate({ to: '/presets', search: { template: next.id, section: 'overview' } })
    },
  })
  const availability = useMutation({
    mutationFn: (target: TemplateRecord) =>
      api(
        `/task-templates/${encodeURIComponent(target.id)}/${target.archived_at ? 'restore' : 'archive'}`,
        'POST',
        {
          expected_revision: target.revision,
        },
      ),
    onSuccess: () => client.invalidateQueries({ queryKey: ['task-templates'] }),
    onError: (error: Error) => toast(error.message),
  })
  const [saveError, setSaveError] = useState('')
  const choose = async (id: string) => {
    if (await requestLeave()) navigate({ to: '/presets', search: { template: id, section } })
  }
  const create = async (kind: NewPreset) => {
    if (await requestLeave())
      navigate({ to: '/presets', search: { new: kind, section: 'definition' } })
  }
  const saveDefinition = () => {
    if (!definition) return
    setSaveError('')
    save.mutate(definition, { onError: (error) => setSaveError(error.message) })
  }
  const listState = list.error ? (
    <ErrorToast>
      {list.error.message}
      <Button onClick={() => list.refetch()}>{t('重试')}</Button>
    </ErrorToast>
  ) : list.isPending ? (
    <p className="quiet padded">{t('正在加载…')}</p>
  ) : !list.data?.items.length ? (
    <p className="quiet padded">{query ? t('无匹配结果') : t('preset.empty')}</p>
  ) : null
  return (
    <>
      <Split
        label={t('preset.title')}
        crumbs={
          record
            ? [{ label: record.definition.name }]
            : search.new
              ? [{ label: t('taskTemplate.newTemplate') }]
              : []
        }
        detailOpen={!!search.template || !!search.new}
        onBack={async () => {
          if (await requestLeave()) navigate({ to: '/presets', search: {} })
        }}
        list={
          <>
            <div className="pane-heading">
              <h1>{t('preset.title')}</h1>
              {editable && (
                <Dropdown.Root>
                  <Dropdown.Trigger asChild>
                    <IconButton label={t('preset.new')}>
                      <ActionIcon name="add" />
                    </IconButton>
                  </Dropdown.Trigger>
                  <Dropdown.Portal>
                    <Dropdown.Content className="dropdown" align="end">
                      <Dropdown.Item onSelect={() => create('task')}>{t('任务')}</Dropdown.Item>
                      <Dropdown.Item onSelect={() => create('dag')}>
                        {t('taskTemplate.dynamicDag')}
                      </Dropdown.Item>
                    </Dropdown.Content>
                  </Dropdown.Portal>
                </Dropdown.Root>
              )}
            </div>
            <div className="preset-list-filter">
              <input
                type="search"
                aria-label={t('taskTemplate.search')}
                placeholder={t('taskTemplate.search')}
                value={query}
                onChange={(event) => {
                  setQuery(event.target.value)
                  setPage(1)
                }}
              />
              <IconButton
                label={archived ? t('preset.showActive') : t('preset.showArchived')}
                aria-pressed={archived}
                onClick={() => {
                  setArchived((value) => !value)
                  setPage(1)
                }}
              >
                <ActionIcon name="archive" />
              </IconButton>
            </div>
            <div className="list-scroll">
              {listState ??
                list.data?.items.map((row) => (
                  <button
                    key={row.id}
                    className={`task-row preset-row ${record?.id === row.id ? 'active' : ''}`}
                    aria-current={record?.id === row.id ? 'page' : undefined}
                    onClick={() => choose(row.id)}
                  >
                    <div className="row-title">
                      <span title={row.definition.name}>{row.definition.name}</span>
                      {row.archived_at && <small className="preset-archived">{t('已归档')}</small>}
                    </div>
                    <div className="row-meta">
                      <span className="preset-type">
                        {row.definition.task.dag ? 'DAG' : t('任务')} ·{' '}
                        {t('taskOverview.parameterCount', {
                          count: Object.keys(row.definition.inputs.properties ?? {}).length,
                        })}
                      </span>
                      <ChevronRight size={13} />
                    </div>
                  </button>
                ))}
            </div>
            <Pagination
              page={page}
              total={list.data?.total ?? 0}
              pending={list.isFetching}
              onChange={setPage}
            />
          </>
        }
      >
        {record || search.new ? (
          <>
            {record && editable && (
              <PageActions>
                <ActionMenu label={t('preset.actions')}>
                  <ActionMenuItem
                    icon={record.archived_at ? 'restore' : 'archive'}
                    disabled={availability.isPending}
                    onSelect={() => availability.mutate(record)}
                  >
                    {record.archived_at ? t('恢复') : t('归档')}
                  </ActionMenuItem>
                </ActionMenu>
              </PageActions>
            )}
            <DetailTabs
              idPrefix={`preset-${record?.id ?? search.new ?? 'new'}`}
              label={t('preset.title')}
              tabs={[
                { key: 'overview', label: t('preset.overview') },
                { key: 'preview', label: t('taskTemplate.preview') },
                { key: 'definition', label: t('preset.definition') },
              ]}
              value={section}
              onChange={(next) =>
                navigate({
                  to: '/presets',
                  search: {
                    template: record?.id,
                    new: search.new,
                    source: search.source,
                    section: next,
                  },
                })
              }
            />
            <DetailBody>
              {section === 'overview' && definition ? (
                <PresetOverview definition={definition} record={record} />
              ) : section === 'preview' ? (
                <>
                  {definitionError && <ErrorToast>{t(definitionError)}</ErrorToast>}
                  <TemplateInstanceView instance={instance} />
                </>
              ) : (
                <ContentCard title={t('preset.definition')}>
                  <AuthorEditor
                    source={source}
                    definition={definition}
                    error={definitionError}
                    onSource={setSource}
                  />
                  {saveError && <ErrorToast>{saveError}</ErrorToast>}
                  {editable && (
                    <div className="preset-editor-actions">
                      <Button
                        variant="solid"
                        loading={save.isPending}
                        disabled={
                          save.isPending ||
                          !definition ||
                          !!definitionError ||
                          !!record?.archived_at ||
                          (!search.new && !dirty)
                        }
                        onClick={saveDefinition}
                      >
                        {t('taskTemplate.saveTemplate')}
                      </Button>
                    </div>
                  )}
                </ContentCard>
              )}
            </DetailBody>
          </>
        ) : (
          <div className="empty">
            <h3>
              {list.error ? t('加载失败') : list.isPending ? t('正在加载…') : t('preset.empty')}
            </h3>
            {list.error && <p>{list.error.message}</p>}
          </div>
        )}
      </Split>
      <Modal
        open={leaveOpen}
        onOpenChange={(open) => {
          if (!open) finishLeave(false)
        }}
        title={t('有未保存的更改')}
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

function PresetOverview({
  definition,
  record,
}: {
  definition: Definition
  record: TemplateRecord | null
}) {
  useTranslation()
  const inputs = Object.entries(definition.inputs.properties ?? {})
  const bindings = Object.entries(definition.bindings)
  return (
    <div className="preset-overview">
      <ContentCard title={t('preset.overview')}>
        <dl className="properties">
          <dt>{t('taskTemplate.description')}</dt>
          <dd>{definition.description || '—'}</dd>
          <dt>{t('类型')}</dt>
          <dd>{definition.task.dag ? t('taskTemplate.dynamicDag') : t('任务')}</dd>
          <dt>{t('taskTemplate.inputs')}</dt>
          <dd>{t('taskOverview.parameterCount', { count: inputs.length })}</dd>
          <dt>{t('taskTemplate.bindings')}</dt>
          <dd>{bindings.length}</dd>
          <dt>Revision</dt>
          <dd>{record ? `r${record.revision}` : '—'}</dd>
          {record?.updated_at && (
            <>
              <dt>{t('更新时间')}</dt>
              <dd>{time(record.updated_at)}</dd>
            </>
          )}
          {record?.archived_at && (
            <>
              <dt>{t('状态')}</dt>
              <dd>{t('已归档')}</dd>
            </>
          )}
        </dl>
      </ContentCard>
      <ContentCard title={t('taskTemplate.inputs')}>
        {inputs.length ? (
          <div className="preset-schema-table" role="table" aria-label={t('taskTemplate.inputs')}>
            <div className="preset-schema-head" role="row">
              <span>{t('名称')}</span>
              <span>{t('类型')}</span>
              <span>{t('taskTemplate.required')}</span>
              <span>{t('taskTemplate.useDefault')}</span>
            </div>
            {inputs.map(([name, input]) => (
              <div role="row" key={name}>
                <span title={input.description ?? name}>
                  {input.title || name}
                  <code> {name}</code>
                </span>
                <span>{Array.isArray(input.type) ? input.type.join(' | ') : input.type}</span>
                <span>{definition.inputs.required?.includes(name) ? '✓' : '—'}</span>
                <span className="preset-default">
                  {'default' in input ? JSON.stringify(input.default) : '—'}
                </span>
              </div>
            ))}
          </div>
        ) : (
          <p className="quiet padded">{t('preset.noInputs')}</p>
        )}
      </ContentCard>
      <ContentCard title={t('taskTemplate.bindings')}>
        {bindings.length ? (
          <div className="preset-schema-table" role="table" aria-label={t('taskTemplate.bindings')}>
            <div className="preset-schema-head" role="row">
              <span>{t('名称')}</span>
              <span>{t('设备')}</span>
              <span>Agent</span>
              <span>{t('taskTemplate.overrides')}</span>
            </div>
            {bindings.map(([name, binding]) => (
              <div role="row" key={name}>
                <span>{binding.title || name}</span>
                <span>{binding.defaults?.runner || '—'}</span>
                <span>{binding.defaults?.agent || '—'}</span>
                <span className="preset-default">
                  {binding.defaults?.parameters ? JSON.stringify(binding.defaults.parameters) : '—'}
                </span>
              </div>
            ))}
          </div>
        ) : (
          <p className="quiet padded">{t('preset.noBindings')}</p>
        )}
      </ContentCard>
    </div>
  )
}
