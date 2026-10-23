import { ActionIcon } from '../actions/ActionIcon'
import { ErrorToast } from '../Toast'
import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { RunnerStatus } from './RunnerStatus'
import { PageActions } from '../PageHeader'
import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate, useSearch } from '@tanstack/react-router'
import * as Dropdown from '@radix-ui/react-dropdown-menu'
import { ChevronRight, Gauge } from '../icons'
import { api, type Runner, type User, type WorkspaceSettings } from '../api'
import { useFilters, runnerMatches, agentMatches } from '../Search'
import { Split } from '../Workbench'
import { Pagination } from '../Pagination'
import { Button, IconButton, Modal } from '../ui'
import { TaskEditor } from '../TaskEditor'
import { RunnerOverview } from './RunnerOverview'
import { RunnerAgents } from './RunnerAgents'
import { EnrollDialog } from './EnrollDialog'

export function RunnersPage({ user, toast }: { user: User; toast: (message: string) => void }) {
  useTranslation()
  const navigate = useNavigate()
  const search = useSearch({ from: '/runners' })
  const client = useQueryClient()
  const filters = useFilters('runners')
  const agentFilters = useFilters('agents')
  const filteringAgents = Object.values(agentFilters).some(Boolean)
  const runners = useQuery<Runner[]>({
    queryKey: ['runners'],
    queryFn: () => api('/runners'),
    refetchInterval: 3000,
  })
  const settings = useQuery<WorkspaceSettings>({
    queryKey: ['settings'],
    queryFn: () => api('/settings'),
    enabled: user.role !== 'viewer',
  })
  const [page, setPage] = useState(1)
  const [enroll, setEnroll] = useState(false)
  const [revokeTarget, setRevokeTarget] = useState<Runner | null>(null)
  const [editor, setEditor] = useState<{ runner: string; agent?: string } | null>(null)
  const filtered = (runners.data ?? [])
    .filter(
      (r) =>
        runnerMatches(r, filters) &&
        (!filteringAgents ||
          r.agents.some((a) =>
            agentMatches({ ...a, runner: r.name, runner_id: r.id }, agentFilters),
          )),
    )
    .sort((a, b) => a.name.localeCompare(b.name) || a.id.localeCompare(b.id))
  const targetIndex = filtered.findIndex((r) => r.id === search.runner)
  const currentPage =
    targetIndex >= 0
      ? Math.floor(targetIndex / 20) + 1
      : Math.min(page, Math.max(1, Math.ceil(filtered.length / 20)))
  const items = filtered.slice((currentPage - 1) * 20, currentPage * 20)
  const runner = filtered.find((r) => r.id === search.runner) ?? items[0]
  const section = search.section ?? (filteringAgents ? 'agents' : 'overview')
  const admin = user.role === 'admin'
  const editable = user.role !== 'viewer'
  const choose = (id: string, nextSection: string = section, agent?: string) =>
    navigate({
      to: '/runners',
      search: { runner: id, section: nextSection as 'overview' | 'agents', agent },
    })
  const revoke = useMutation({
    mutationFn: (id: string) => api(`/runners/${id}/revoke`, 'POST', {}),
    onSuccess: () => {
      client.invalidateQueries({ queryKey: ['runners'] })
      setRevokeTarget(null)
    },
    onError: (e) => toast(e.message),
  })
  const listState = runners.error ? (
    <ErrorToast>
      {runners.error.message}
      <Button onClick={() => runners.refetch()}>{tr('重试')}</Button>
    </ErrorToast>
  ) : runners.isPending ? (
    <p className="quiet padded">{tr('正在加载…')}</p>
  ) : !filtered.length ? (
    <p className="quiet padded">{runners.data?.length ? tr('无匹配设备') : tr('暂无设备')}</p>
  ) : null
  return (
    <>
      <Split
        label={tr('Runners')}
        crumbs={
          runner
            ? [
                { label: runner.name, onClick: () => choose(runner.id, 'overview') },
                ...(section === 'agents'
                  ? [
                      { label: tr('Agents'), onClick: () => choose(runner.id, 'agents') },
                      ...runner.agents
                        .filter((a) => a.id === search.agent)
                        .map((a) => ({ label: a.name })),
                    ]
                  : []),
              ]
            : []
        }
        detailOpen={!!search.runner}
        onBack={() => navigate({ to: '/runners', search: {} })}
        list={
          <>
            <div className="pane-heading">
              <h1>{tr('Runners')}</h1>
              {editable && (
                <IconButton label={tr('连接设备')} onClick={() => setEnroll(true)}>
                  <ActionIcon name="add" />
                </IconButton>
              )}
            </div>
            <div className="list-scroll">
              {listState ??
                items.map((r) => (
                  <button
                    key={r.id}
                    className={`task-row runner-row ${runner?.id === r.id ? 'active' : ''}`}
                    aria-current={runner?.id === r.id ? 'page' : undefined}
                    onClick={() => choose(r.id)}
                  >
                    <div className="row-title">
                      <span title={r.name}>{r.name}</span>
                      <RunnerStatus runner={r} />
                    </div>
                    <div className="row-meta">
                      <span>{r.info.os ?? '—'}</span>

                      <span className="runner-capacity">
                        <Gauge size={12} />
                        {(r.info.active ?? 0) + (r.info.unknown ?? 0)} / {r.info.concurrency ?? '—'}
                      </span>
                      <ChevronRight size={13} />
                    </div>
                  </button>
                ))}
            </div>
            <Pagination
              page={currentPage}
              total={filtered.length}
              pending={runners.isPending}
              onChange={(p) => {
                setPage(p)
                const first = filtered[(p - 1) * 20]
                if (first) choose(first.id)
              }}
            />
          </>
        }
      >
        {runner ? (
          <>
            <PageActions>
              {editable && (
                <IconButton
                  label={tr('创建任务')}
                  text={tr('创建任务')}
                  disabled={runner.revoked || !settings.data}
                  onClick={() => setEditor({ runner: runner.name })}
                >
                  <ActionIcon name="add" />
                </IconButton>
              )}
              {admin && !runner.revoked && (
                <Dropdown.Root>
                  <Dropdown.Trigger asChild>
                    <IconButton label={tr('设备操作')}>
                      <ActionIcon name="more" />
                    </IconButton>
                  </Dropdown.Trigger>
                  <Dropdown.Portal>
                    <Dropdown.Content className="dropdown" align="end">
                      <Dropdown.Item className="danger" onSelect={() => setRevokeTarget(runner)}>
                        {tr('吊销设备')}
                      </Dropdown.Item>
                    </Dropdown.Content>
                  </Dropdown.Portal>
                </Dropdown.Root>
              )}
            </PageActions>
            <div className="tabs" role="tablist" aria-label={tr('设备详情')}>
              {[
                ['overview', tr('概览')],
                ['agents', tr('Agents')],
              ].map(([id, label]) => (
                <button
                  key={id}
                  role="tab"
                  aria-selected={section === id}
                  className={section === id ? 'active' : ''}
                  onClick={() => choose(runner.id, id)}
                >
                  {label}
                </button>
              ))}
            </div>
            {section === 'agents' ? (
              <RunnerAgents
                runner={runner}
                filters={filteringAgents ? agentFilters : {}}
                expanded={search.agent}
                onExpand={(id) => choose(runner.id, 'agents', id)}
                onCreate={
                  editable ? (agent) => setEditor({ runner: runner.name, agent }) : undefined
                }
                createReady={!!settings.data}
              />
            ) : (
              <RunnerOverview runner={runner} />
            )}
          </>
        ) : (
          <div className="empty">
            <h3>
              {runners.error
                ? tr('无法加载设备')
                : runners.isPending
                  ? tr('正在加载…')
                  : runners.data?.length
                    ? tr('无匹配设备')
                    : tr('暂无设备')}
            </h3>
          </div>
        )}
      </Split>
      {enroll && <EnrollDialog onClose={() => setEnroll(false)} toast={toast} />}
      {editor && settings.data && (
        <TaskEditor
          defaults={editor}
          defaultTimezone={settings.data.timezone}
          onClose={() => setEditor(null)}
        />
      )}
      <Modal
        open={!!revokeTarget}
        onOpenChange={(v) => !v && setRevokeTarget(null)}
        title={tr('吊销设备')}
        description={tr('停止 {{v0}} 的新任务领取，并撤销设备访问权限。', {
          v0: revokeTarget?.name ?? '',
        })}
      >
        <div className="modal-body">
          <p>{tr('正在运行的任务会标记为待核对，需要检查设备上的实际执行状态。')}</p>
        </div>
        <div className="modal-footer">
          <Button onClick={() => setRevokeTarget(null)}>{tr('取消')}</Button>
          <Button
            variant="solid"
            danger
            disabled={revoke.isPending}
            onClick={() => revokeTarget && revoke.mutate(revokeTarget.id)}
          >
            {tr('吊销设备')}
          </Button>
        </div>
      </Modal>
    </>
  )
}
