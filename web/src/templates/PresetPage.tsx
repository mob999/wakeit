import { useEffect, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useNavigate, useSearch } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { api, type Runner, type User } from '../api'
import { t } from '../i18n'
import { useFilters } from '../Search'
import { Split, DetailBody } from '../Workbench'
import { DetailTabs } from '../DetailTabs'
import { PageActions } from '../PageHeader'
import { ActionIcon } from '../actions/ActionIcon'
import { Button, IconButton } from '../ui'
import { ErrorToast } from '../Toast'
import { Pagination } from '../Pagination'
import { CreateTaskDialog } from '../CreateTaskDialog'
import type { TemplateRecord } from './types'
import { PresetOverview } from './PresetOverview'
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
                <ErrorToast
                  error={list.error.message}
                  actions={<Button onClick={() => list.refetch()}>{t('重试')}</Button>}
                />
              ) : list.isPending ? (
                <p className="quiet text-muted padded p-5">{t('正在加载…')}</p>
              ) : !list.data?.items.length ? (
                <p className="quiet text-muted padded p-5">
                  {t(filters.q ? '无匹配结果' : 'preset.empty')}
                </p>
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
            <ErrorToast
              error={failed.message}
              actions={
                <Button onClick={() => (search.template ? selected.refetch() : list.refetch())}>
                  {t('重试')}
                </Button>
              }
            />
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
              <PresetOverview record={record} runners={runners.data ?? []} />
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
