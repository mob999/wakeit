import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { api, terminal, type Run, type User, type Agent, type WorkspaceSettings } from '../api'
import { t } from '../i18n'
import { PageActions } from '../PageHeader'
import { IconButton } from '../ui'
import { ActionIcon } from '../actions/ActionIcon'
import { ActionMenu, ActionMenuItem, CopyMenuItem } from '../actions/ActionMenu'
import { ResumeRun } from '../sessions/ResumeRun'
import { CreateTaskDialog } from '../CreateTaskDialog'
import { DagRunActionDialog, type DagAction } from '../dag/DagRunActionDialog'
import { ResolveDialog } from './RunDetail'

export function RunActions({
  run: r,
  user,
  agents,
  toast,
}: {
  run: Run
  user: User
  agents?: Agent[]
  toast: (message: string) => void
}) {
  useTranslation()
  const client = useQueryClient()
  const navigate = useNavigate()
  const [picker, setPicker] = useState<DagAction | null>(null)
  const [resolve, setResolve] = useState<string | null>(null)
  const [source, setSource] = useState<Run | null>(null)
  const settings = useQuery<WorkspaceSettings>({
    queryKey: ['settings'],
    queryFn: () => api('/settings'),
    enabled: !!source,
  })
  useEffect(() => {
    if (source && settings.error) {
      toast(settings.error.message)
      setSource(null)
    }
  }, [source, settings.error, toast])
  const action = useMutation({
    mutationFn: (type: 'cancel' | 'retry') =>
      api<{ id?: string }>(`/runs/${r.id}/${type}`, 'POST', {}),
    onSuccess: (value) => {
      client.invalidateQueries({ queryKey: ['run', r.id] })
      if (value.id) navigate({ to: '/runs', search: { run: value.id } })
    },
    onError: (error) => toast(error.message),
  })
  const dag = !!r.config.dag
  const editable = !r.parent_run_id && user.role !== 'viewer'
  const sessionReady = dag
    ? r.nodes?.some((n) => n.continuation_available && terminal(n.state))
    : terminal(r.state) && !!r.session?.ready
  const cancellable =
    dag ||
    r.state === 'queued' ||
    agents?.find(
      (a) =>
        a.id === r.config.agent &&
        (a.runner === r.config.runner || a.runner_id === r.config.runner),
    )?.cancellable
  const canCancel = dag ? !terminal(r.state) : ['queued', 'starting', 'running'].includes(r.state)
  const unknown = dag ? r.nodes?.some((n) => n.state === 'unknown') : r.state === 'unknown'
  return (
    <>
      <PageActions>
        <ResumeRun
          run={r}
          id={r.id}
          available={!!sessionReady}
          selectNodeFirst={dag}
          toast={toast}
        />
        <div className="action-group" role="group" aria-label={t('actions.execution')}>
          {editable && canCancel && (
            <IconButton
              label={t('取消执行')}
              danger
              disabled={!cancellable || r.cancel_requested || action.isPending}
              disabledReason={
                r.cancel_requested
                  ? t('取消已请求')
                  : !cancellable
                    ? t('actions.cancelUnavailable')
                    : undefined
              }
              loading={action.isPending && action.variables === 'cancel'}
              onClick={() => action.mutate('cancel')}
            >
              <ActionIcon name="stop" />
            </IconButton>
          )}
          {editable && !r.task_archived && (dag || terminal(r.state)) && (
            <IconButton
              label={t('actions.rerun')}
              loading={action.isPending && action.variables === 'retry'}
              disabled={action.isPending}
              onClick={() => (dag ? setPicker('retry') : action.mutate('retry'))}
            >
              <ActionIcon name="retry" />
            </IconButton>
          )}
          {editable && unknown && (
            <IconButton
              label={t('核对状态')}
              onClick={() => (dag ? setPicker('resolve') : setResolve(''))}
            >
              <ActionIcon name="reconcile" />
            </IconButton>
          )}
        </div>
        <ActionMenu label={t('actions.runMenu')}>
          {editable && sessionReady && (
            <ActionMenuItem
              icon="add"
              disabled={!!source && settings.isPending}
              onSelect={() => (dag ? setPicker('task') : setSource(r))}
            >
              {t('从此会话创建任务')}
            </ActionMenuItem>
          )}
          <CopyMenuItem label={t('复制 Run ID')} value={r.id} toast={toast} />
        </ActionMenu>
      </PageActions>
      {picker && (
        <DagRunActionDialog
          run={r}
          kind={picker}
          onClose={() => setPicker(null)}
          toast={toast}
          onSession={(execution) => {
            setPicker(null)
            setSource(execution)
          }}
          onResolve={(node) => {
            setPicker(null)
            setResolve(node)
          }}
        />
      )}
      {resolve !== null && (
        <ResolveDialog
          id={r.id}
          endpoint={dag ? `/runs/${r.id}/nodes/${encodeURIComponent(resolve)}/resolve` : undefined}
          onClose={() => setResolve(null)}
        />
      )}
      {source?.session?.ready && settings.data && (
        <CreateTaskDialog
          timezone={settings.data.timezone}
          defaults={{
            runner: source.session.runner_id,
            agent: source.session.agent,
            cwd: source.session.cwd,
            parameters: source.execution_snapshot?.parameters ?? source.config.parameters,
            session: { id: source.session.id },
          }}
          onClose={() => setSource(null)}
        />
      )}
    </>
  )
}
