import { useEffect, useState } from 'react'
import { useQuery, useQueries, useMutation, useQueryClient } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { api, terminal, stateNames, type Run } from '../api'
import { apiErrorText } from '../apiErrorText'
import { t } from '../i18n'
import { Button, Modal } from '../ui'
import { RunNodeList } from '../runs/RunNodeList'

export type DagAction = 'retry' | 'task' | 'resolve'
type NodeAction = {
  execution: Run | null
  retry?: {
    action: 'retry' | 'accelerate' | 'resume' | null
    reason?: string
    affected: { id: string; name: string; state: string }[]
  }
}

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
  const [target, setTarget] = useState('')
  const client = useQueryClient()
  const navigate = useNavigate()
  const nodeId = target.startsWith('node:') ? target.slice(5) : ''
  const node = run.nodes?.find((n) => n.id === nodeId)
  const graph = target === 'graph'
  const detail = useQuery<NodeAction>({
    queryKey: ['dag-action', run.id, nodeId],
    queryFn: () => api(`/runs/${run.id}/nodes/${encodeURIComponent(nodeId)}`),
    enabled: !!nodeId,
    // Eligibility must be current; never reuse another selection's execution ID.
    staleTime: 0,
    refetchInterval: 2000,
  })
  const eligibility = useQueries({
    queries: (run.nodes ?? []).map((node) => ({
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
  const resume = !graph && retry?.action === 'resume'
  const blocked = graph
    ? run.task_archived
      ? t('Task is archived')
      : !terminal(run.state)
        ? t('runActions.graphRunning')
        : ''
    : retry?.reason
      ? apiErrorText(retry.reason, t, (state) => stateNames[state] ?? state)
      : ''
  const ready =
    !!target &&
    (graph
      ? !blocked
      : !!node &&
        !detail.isPending &&
        !detail.isError &&
        !!detail.data &&
        (kind === 'retry'
          ? !!retry?.action && !blocked
          : kind === 'task'
            ? !!detail.data.execution?.session?.ready && terminal(detail.data.execution.state)
            : detail.data.execution?.state === 'unknown'))
  const action = useMutation({
    mutationFn: () =>
      graph
        ? api<{ id?: string }>(`/runs/${run.id}/retry`, 'POST', {})
        : api<{ id?: string }>(
            `/runs/${run.id}/nodes/${encodeURIComponent(nodeId)}/retry`,
            'POST',
            {
              execution_id: detail.data?.execution?.id ?? null,
              resume_stopped: resume,
            },
          ),
    onSuccess: (value) => {
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
      if (graph && value.id) navigate({ to: '/runs', search: { run: value.id } })
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
          : retry?.action === 'accelerate' && !graph
            ? t('retry.now')
            : graph
              ? t('runActions.entireGraph')
              : t('actions.rerun')
  function submit() {
    if (!ready || action.isPending) return
    if (kind === 'task') onSession(detail.data!.execution!)
    else if (kind === 'resolve') onResolve(nodeId)
    else action.mutate()
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
          value={target}
          disabled={action.isPending}
          onChange={setTarget}
          options={[
            ...(kind === 'retry'
              ? [
                  {
                    value: 'graph',
                    all: true,
                    name: t('runActions.entireGraph'),
                    disabled: !terminal(run.state) || run.task_archived,
                    reason: run.task_archived
                      ? t('Task is archived')
                      : !terminal(run.state)
                        ? t('runActions.graphRunning')
                        : undefined,
                  },
                ]
              : []),
            ...(run.nodes ?? []).map((node, index) => {
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
                value: `node:${node.id}`,
                name: node.name,
                state: node.state,
                reason,
                disabled: kind === 'retry' ? !policy?.action || !!query.error : !!reason,
                pending: kind === 'retry' && query.isPending,
              }
            }),
          ]}
        />
      </div>
      <div className="modal-footer">
        <Button disabled={action.isPending} onClick={onClose}>
          {t('取消')}
        </Button>
        <Button className="primary" disabled={!ready} loading={action.isPending} onClick={submit}>
          {label}
        </Button>
      </div>
    </Modal>
  )
}
