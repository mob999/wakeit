import { useEffect, useState } from 'react'
import { useQuery, useQueries, useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { api } from '../api/client'
import { terminal } from '../runs/state'
import { stateNames } from '../i18n'
import { type Run } from '../api/types'
import { apiErrorText } from '../api/errorText'
import { t } from '../i18n'
import { Button, IconButton, Modal } from '../components/ui'
import { ActionIcon } from '../actions/ActionIcon'
import { RunNodeList, RUN_NODE_PAGE_SIZE } from '../runs/RunNodeList'

export type DagAction = 'retry' | 'task' | 'resolve'
type NodeAction = {
  execution: Run | null
  retry?: {
    action: 'retry' | 'accelerate' | 'resume' | null
    reason?: string
    affected: { id: string; name: string; state: string }[]
  }
}
type RetryTarget =
  { scope: 'graph' } | { scope: 'node'; node: string; executionId: string | null; resume: boolean }

export function DagRunActionDialog({
  run,
  kind,
  onClose,
  onSession,
  onResolve,
  toast,
}: {
  run: Run
  kind: DagAction
  onClose: () => void
  onSession: (run: Run) => void
  onResolve: (node: string) => void
  toast: (message: string) => void
}) {
  useTranslation()
  const [nodeId, setNodeId] = useState('')
  const [page, setPage] = useState(1)
  const client = useQueryClient()
  const navigate = useNavigate()
  const nodes = run.nodes ?? []
  const pageNodes = nodes.slice((page - 1) * RUN_NODE_PAGE_SIZE, page * RUN_NODE_PAGE_SIZE)
  const node = pageNodes.find((n) => n.id === nodeId)
  const detail = useQuery<NodeAction>({
    queryKey: ['dag-action', run.id, nodeId],
    queryFn: () => api(`/runs/${run.id}/nodes/${encodeURIComponent(nodeId)}`),
    enabled: !!nodeId,
    // Eligibility must be current; never reuse another selection's execution ID.
    staleTime: 0,
    refetchInterval: 2000,
  })
  const eligibility = useQueries({
    queries: pageNodes.map((node) => ({
      queryKey: ['dag-action', run.id, node.id],
      queryFn: () => api<NodeAction>(`/runs/${run.id}/nodes/${encodeURIComponent(node.id)}`),
      enabled: kind === 'retry',
      staleTime: 0,
    })),
  })
  const loadError = detail.error ?? eligibility.find((query) => query.error)?.error
  useEffect(() => {
    if (loadError) toast(loadError.message)
  }, [loadError, toast])
  const retry = detail.data?.retry
  const resume = retry?.action === 'resume'
  const graphBlocked = run.task_archived
    ? t('Task is archived')
    : !terminal(run.state)
      ? t('runActions.graphRunning')
      : undefined
  const blocked = retry?.reason
    ? apiErrorText(retry.reason, t, (state) => stateNames[state] ?? state)
    : ''
  const ready =
    !!node &&
    !detail.isPending &&
    !detail.isError &&
    !!detail.data &&
    (kind === 'retry'
      ? !!retry?.action && !blocked
      : kind === 'task'
        ? !!detail.data.execution?.session?.ready && terminal(detail.data.execution.state)
        : detail.data.execution?.state === 'unknown')
  const action = useMutation({
    mutationFn: (target: RetryTarget) =>
      target.scope === 'graph'
        ? api<{ id?: string }>(`/runs/${run.id}/retry`, 'POST', {})
        : api<{ id?: string }>(
            `/runs/${run.id}/nodes/${encodeURIComponent(target.node)}/retry`,
            'POST',
            {
              execution_id: target.executionId,
              resume_stopped: target.resume,
            },
          ),
    onSuccess: (value, target) => {
      for (const queryKey of [
        ['run', run.id],
        ['runs'],
        ['moments'],
        ['search'],
        ['dag-node', run.id],
        ['dag-action', run.id],
      ])
        client.invalidateQueries({ queryKey })
      onClose()
      if (target.scope === 'graph' && value.id) navigate({ to: '/runs', search: { run: value.id } })
    },
    onError: (error) => {
      toast(error.message)
      client.invalidateQueries({ queryKey: ['dag-action', run.id] })
      client.invalidateQueries({ queryKey: ['run', run.id] })
    },
  })
  const label =
    kind === 'task'
      ? t('从此会话创建任务')
      : kind === 'resolve'
        ? t('核对状态')
        : resume
          ? t('retry.resume')
          : retry?.action === 'accelerate'
            ? t('retry.now')
            : t('retry.node')
  function submit() {
    if (!ready || action.isPending) return
    if (kind === 'task') onSession(detail.data!.execution!)
    else if (kind === 'resolve') onResolve(nodeId)
    else
      action.mutate({
        scope: 'node',
        node: nodeId,
        executionId: detail.data?.execution?.id ?? null,
        resume,
      })
  }
  return (
    <Modal
      open
      title={kind === 'retry' ? t('actions.rerun') : label}
      onOpenChange={(open) => {
        if (!open && !action.isPending) onClose()
      }}
      className="run-node-modal"
    >
      <div className="modal-body">
        <RunNodeList
          value={nodeId}
          disabled={action.isPending}
          onChange={setNodeId}
          page={page}
          total={nodes.length}
          onPageChange={(next) => {
            setNodeId('')
            setPage(next)
          }}
          options={pageNodes.map((node, index) => {
            const query = eligibility[index]
            const policy = query.data?.retry
            const reason =
              kind === 'retry'
                ? policy?.reason
                  ? apiErrorText(policy.reason, t, (state) => stateNames[state] ?? state)
                  : undefined
                : kind === 'task' && (!node.continuation_available || !terminal(node.state))
                  ? t('该会话暂不可续聊')
                  : kind === 'resolve' && node.state !== 'unknown'
                    ? t('runActions.notUnknown')
                    : undefined
            return {
              value: node.id,
              name: node.name,
              state: node.state,
              reason,
              disabled: kind === 'retry' ? !policy?.action || !!query.error : !!reason,
              pending: kind === 'retry' && query.isPending,
            }
          })}
        />
      </div>
      <div className="modal-footer">
        {kind === 'retry' && (
          <div className="run-node-all-action">
            <IconButton
              label={t('runActions.entireGraph')}
              text={t('runActions.entireGraph')}
              disabled={!!graphBlocked || action.isPending}
              disabledReason={graphBlocked}
              loading={action.isPending && action.variables?.scope === 'graph'}
              onClick={() => {
                if (!graphBlocked && !action.isPending) action.mutate({ scope: 'graph' })
              }}
            >
              <ActionIcon name="retry" />
            </IconButton>
          </div>
        )}
        <Button disabled={action.isPending} onClick={onClose}>
          {t('取消')}
        </Button>
        <Button
          className="primary"
          disabled={!ready || action.isPending}
          loading={action.isPending && action.variables?.scope === 'node'}
          onClick={submit}
        >
          {label}
        </Button>
      </div>
    </Modal>
  )
}
