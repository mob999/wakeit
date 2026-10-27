import YAML from 'yaml'
import { useEffect, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useNavigate, useSearch } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { api, time, type Runner, type User } from '../api'
import { t } from '../i18n'
import { useFilters } from '../Search'
import { Split, DetailBody } from '../Workbench'
import { DetailTabs } from '../DetailTabs'
import { PageActions } from '../PageHeader'
import { ActionIcon } from '../actions/ActionIcon'
import { Button, IconButton } from '../ui'
import { ContentCard } from '../ContentCard'
import { ErrorToast } from '../Toast'
import { Pagination } from '../Pagination'
import { CreateTaskDialog } from '../CreateTaskDialog'
import type { InputSchema, TemplateRecord } from './types'
import './templates.css'

export function PresetPage({ user }: { user: User }) {
  useTranslation()
  const navigate = useNavigate()
  const search = useSearch({ from: '/presets' })
  const filters = useFilters('presets')
  const [page, setPage] = useState(1)
  const [creating, setCreating] = useState(false)
  const editable = user.role !== 'viewer' && !user.workspace?.is_demo
  useEffect(() => setPage(1), [filters.q])
  const list = useQuery<{ items: TemplateRecord[]; total: number }>({
    queryKey: ['task-templates', 'list', filters.q, page],
    queryFn: () =>
      api(`/task-templates?q=${encodeURIComponent(filters.q ?? '')}&archived=false&page=${page}`),
    refetchInterval: 10000,
  })
  const selected = useQuery<TemplateRecord>({
    queryKey: ['task-templates', 'detail', search.template],
    queryFn: () => api(`/task-templates/${encodeURIComponent(search.template!)}`),
    enabled: !!search.template,
    retry: false,
  })
  const record = search.template ? selected.data : list.data?.items[0]
  const runners = useQuery<Runner[]>({ queryKey: ['runners'], queryFn: () => api('/runners') })
  const settings = useQuery<{ timezone: string }>({
    queryKey: ['settings'],
    queryFn: () => api('/settings'),
  })
  const failed = search.template ? selected.error : list.error
  const loading = search.template ? selected.isPending : list.isPending
  const definition = record?.definition
  const runnerName = (id?: string | null) =>
    runners.data?.find((runner) => runner.id === id || runner.name === id)?.name || id || '—'
  return (
    <>
      <Split
        label={t('preset.title')}
        crumbs={definition ? [{ label: definition.name }] : []}
        detailOpen={!!search.template}
        onBack={() => navigate({ to: '/presets', search: {} })}
        list={
          <>
            <div className="pane-heading">
              <h1>{t('preset.title')}</h1>
            </div>
            <div className="list-scroll">
              {list.error ? (
                <ErrorToast>
                  {list.error.message}
                  <Button onClick={() => list.refetch()}>{t('重试')}</Button>
                </ErrorToast>
              ) : list.isPending ? (
                <p className="quiet padded">{t('正在加载…')}</p>
              ) : !list.data?.items.length ? (
                <p className="quiet padded">{t(filters.q ? '无匹配结果' : 'preset.empty')}</p>
              ) : (
                list.data.items.map((row) => (
                  <button
                    key={row.id}
                    className={`task-row preset-row ${record?.id === row.id ? 'active' : ''}`}
                    aria-current={record?.id === row.id ? 'page' : undefined}
                    onClick={() => navigate({ to: '/presets', search: { template: row.id } })}
                  >
                    <div className="row-title">
                      <span title={row.definition.name}>{row.definition.name}</span>
                    </div>
                    <div className="row-meta">
                      <span>
                        {row.definition.task.dag ? 'DAG' : t('任务')} ·{' '}
                        {t('taskOverview.parameterCount', {
                          count: Object.keys(row.definition.inputs.properties ?? {}).length,
                        })}
                      </span>
                    </div>
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
          </>
        }
      >
        {failed ? (
          <div className="empty">
            <ErrorToast>
              {failed.message}
              <Button onClick={() => (search.template ? selected.refetch() : list.refetch())}>
                {t('重试')}
              </Button>
            </ErrorToast>
          </div>
        ) : loading ? (
          <div className="empty" role="status">
            {t('正在加载…')}
          </div>
        ) : record && definition ? (
          <>
            {editable && !record.archived_at && (
              <PageActions>
                <IconButton
                  label={t('creation.usePreset')}
                  disabled={!settings.data}
                  onClick={() => setCreating(true)}
                >
                  <ActionIcon name="add" />
                </IconButton>
              </PageActions>
            )}
            <DetailTabs
              idPrefix="preset"
              label={t('preset.title')}
              tabs={[{ key: 'overview', label: t('preset.overview') }]}
              value="overview"
              onChange={() => {}}
            />
            <DetailBody>
              <ContentCard title={definition.name}>
                <div className="preset-readonly">
                  <div className="preset-summary">
                    <p>{definition.description || '—'}</p>
                    <div className="quiet">
                      <span>{definition.task.dag ? 'DAG' : t('任务')}</span>
                      <span>{t('creation.revision', { count: record.revision })}</span>
                      {record.updated_at && <span>{time(record.updated_at)}</span>}
                      {record.archived_at && <span>{t('已归档')}</span>}
                    </div>
                  </div>
                  <section>
                    <h3>{t('taskTemplate.inputs')}</h3>
                    {Object.keys(definition.inputs.properties ?? {}).length ? (
                      <div className="preset-table-scroll">
                        <table className="preset-schema-table">
                          <thead>
                            <tr>
                              <th>{t('名称')}</th>
                              <th>{t('类型')}</th>
                              <th>{t('creation.default')}</th>
                            </tr>
                          </thead>
                          <tbody>
                            {Object.entries(definition.inputs.properties ?? {}).map(
                              ([name, input]) => (
                                <tr key={name}>
                                  <td>
                                    <strong>
                                      {input.title || name}
                                      {definition.inputs.required?.includes(name) && (
                                        <span aria-label={t('taskTemplate.required')}> *</span>
                                      )}
                                    </strong>
                                    {input.description && <small>{input.description}</small>}
                                  </td>
                                  <td>{typeLabel(input)}</td>
                                  <td>
                                    <span>
                                      {'default' in input ? formatValue(input.default) : '—'}
                                    </span>
                                  </td>
                                </tr>
                              ),
                            )}
                          </tbody>
                        </table>
                      </div>
                    ) : (
                      <p className="quiet padded">{t('preset.noInputs')}</p>
                    )}
                  </section>
                  <section>
                    <h3>{t('taskTemplate.bindings')}</h3>
                    {Object.keys(definition.bindings).length ? (
                      <dl className="preset-environments">
                        {Object.entries(definition.bindings).map(([name, binding]) => (
                          <div key={name}>
                            <dt>{binding.title || name}</dt>
                            <dd>
                              {runnerName(binding.defaults?.runner)} ·{' '}
                              {binding.defaults?.agent || '—'}
                            </dd>
                          </div>
                        ))}
                      </dl>
                    ) : (
                      <p className="quiet padded">{t('preset.noBindings')}</p>
                    )}
                  </section>
                  <details className="preset-source">
                    <summary>{t('creation.viewDefinition')}</summary>
                    <pre className="code">{YAML.stringify(definition.task)}</pre>
                  </details>
                </div>
              </ContentCard>
            </DetailBody>
          </>
        ) : (
          <div className="empty">
            <h3>{t('preset.empty')}</h3>
          </div>
        )}
      </Split>
      {creating && record && (
        <CreateTaskDialog
          timezone={settings.data?.timezone ?? 'Asia/Shanghai'}
          initialTab="preset"
          presetId={record.id}
          onClose={() => setCreating(false)}
          onCreated={(id, runId) =>
            runId
              ? navigate({ to: '/runs', search: { run: runId } })
              : navigate({ to: '/tasks', search: { task: id } })
          }
        />
      )}
    </>
  )
}
function typeLabel(schema: InputSchema) {
  const names: Record<string, string> = {
    string: t('creation.typeText'),
    boolean: t('creation.typeBoolean'),
    integer: t('creation.typeInteger'),
    number: t('creation.typeNumber'),
    array: t('creation.typeList'),
    object: t('creation.typeObject'),
    null: t('creation.nullValue'),
  }
  return (Array.isArray(schema.type) ? schema.type : [schema.type])
    .map((type) => names[type] ?? type)
    .join(' / ')
}
function formatValue(value: unknown) {
  if (value === true || value === false) return t(value ? 'creation.yes' : 'creation.no')
  if (value === null) return t('creation.nullValue')
  return typeof value === 'string' ? value || '""' : JSON.stringify(value)
}
