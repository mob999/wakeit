import { useToast, ErrorToast } from '../components/Toast'
import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { Split } from '../components/Workbench'
import { RunDetail } from './RunDetail'
import { RunListRow } from './RunListRow'
import { useFilters } from '../search/context'
import { listPath } from '../search/filters'
import { type PageResult } from '../api/types'
import { Pagination } from '../components/Pagination'
import { useState, useEffect } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { api } from '../api/client'
import { type User, type Run } from '../api/types'
import { time } from '../i18n'
import { Empty } from '../components/Empty'
import { useAgents } from '../runners/useAgents'

export function RunsPage({ user, search }: { user: User; search: { run?: string } }) {
  useTranslation()
  const toast = useToast()
  const agents = useAgents()
  const navigate = useNavigate()
  const filters = useFilters('runs')
  const [page, setPage] = useState(1)
  useEffect(() => setPage(1), [JSON.stringify(filters)])
  const runs = useQuery<PageResult<Run>>({
    queryKey: ['runs', filters, page],
    queryFn: () => api(listPath('runs', filters, page)),
    refetchInterval: 2000,
  })
  useEffect(() => {
    if (runs.data && page > 1 && (page - 1) * 20 >= runs.data.total)
      setPage(Math.max(1, Math.ceil(runs.data.total / 20)))
  }, [runs.data, page])
  const selected = search.run ?? runs.data?.items[0]?.id
  const detail = useQuery<Run>({
    queryKey: ['run', selected],
    queryFn: () => api(`/runs/${selected}`),
    enabled: !!selected,
    refetchInterval: 2000,
  })
  const selectedRun = runs.data?.items.find((r) => r.id === selected) ?? detail.data
  return (
    <Split
      label={tr('Runs')}
      crumbs={
        selectedRun
          ? [
              {
                label: selectedRun.config.name,
                onClick: () => navigate({ to: '/tasks', search: { task: selectedRun.task_id } }),
              },
              { label: time(selectedRun.started_at ?? selectedRun.created_at) },
            ]
          : []
      }
      detailOpen={!!search.run}
      onBack={() => navigate({ to: '/runs', search: {} })}
      list={
        <>
          <div className="pane-heading">
            <h1>{tr('Runs')}</h1>
          </div>
          <div className="list-scroll">
            {runs.error ? (
              <ErrorToast error={runs.error.message} />
            ) : !runs.data?.items.length ? (
              <Empty title={tr('暂无执行记录')} />
            ) : (
              runs.data?.items.map((r) => (
                <RunListRow
                  key={r.id}
                  run={r}
                  agents={agents.data}
                  selected={selected === r.id}
                  onSelect={() => navigate({ to: '/runs', search: { run: r.id } })}
                />
              ))
            )}
          </div>
          <Pagination
            page={page}
            total={runs.data?.total ?? 0}
            pending={runs.isFetching}
            onChange={(p) => {
              setPage(p)
              navigate({ to: '/runs', search: {} })
            }}
          />
        </>
      }
    >
      {selected ? (
        <RunDetail key={selected} id={selected} user={user} toast={toast} />
      ) : (
        <Empty title={tr('暂无执行记录')} />
      )}
    </Split>
  )
}
