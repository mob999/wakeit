import { useEffect, useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { RotateCcw } from 'lucide-react'
import { api, type Task, type Agent, type Runner } from '../api'
import { type PageResult } from '../Search'
import { Pagination } from '../Pagination'
import { DetailBody } from '../Workbench'
import { AgentLogo } from '../AgentLogo'
import { Button, IconButton, Modal } from '../ui'
import { RequestState } from './shared'
export function ArchivedTasks() {
  const client = useQueryClient()
  const [page, setPage] = useState(1)
  const [selected, setSelected] = useState<Task | null>(null)
  const query = useQuery<PageResult<Task>>({
    queryKey: ['archived-tasks', page],
    queryFn: () => api(`/tasks?archived=true&page=${page}&page_size=20`),
    refetchInterval: 3000,
  })
  const runners = useQuery<Runner[]>({ queryKey: ['runners'], queryFn: () => api('/runners') })
  const agents = useQuery<Agent[]>({ queryKey: ['agents'], queryFn: () => api('/agents') })
  useEffect(() => {
    if (query.data && page > 1 && (page - 1) * 20 >= query.data.total)
      setPage(Math.max(1, Math.ceil(query.data.total / 20)))
  }, [query.data, page])
  const restore = useMutation({
    mutationFn: (task: Task) =>
      api(`/tasks/${task.id}/restore`, 'POST', { expected_revision: task.revision }),
    onSuccess: () => {
      setSelected(null)
      client.invalidateQueries({ queryKey: ['archived-tasks'] })
      client.invalidateQueries({ queryKey: ['tasks'] })
      client.invalidateQueries({ queryKey: ['run'] })
    },
  })
  return (
    <>
      <DetailBody limited>
        {query.error || query.isPending ? (
          <RequestState
            pending={query.isPending}
            error={query.error}
            retry={() => query.refetch()}
          />
        ) : !query.data.items.length ? (
          <div className="settings-request content-card">暂无归档任务</div>
        ) : (
          <div className="content-card archive-table">
            <div className="archive-row table-head">
              <span>任务</span>
              <span>Runner</span>
              <span>Agent</span>
              <span>归档时间</span>
              <span>操作</span>
            </div>
            {query.data.items.map((task) => {
              const agent = agents.data?.find(
                (a) =>
                  a.id === task.config.agent &&
                  (a.runner === task.config.runner || a.runner_id === task.config.runner),
              )
              return (
                <div className="archive-row" key={task.id}>
                  <Link className="archive-name" to="/tasks" search={{ task: task.id }}>
                    {task.config.name}
                  </Link>
                  <span>
                    {runners.data?.find(
                      (r) => r.id === task.config.runner || r.name === task.config.runner,
                    )?.name ?? task.config.runner}
                  </span>
                  <span className="archive-agent">
                    <AgentLogo name={agent?.name ?? task.config.agent} logo={agent?.logo} />
                    {agent?.name ?? task.config.agent}
                  </span>
                  <span className="mono quiet">
                    {task.archived_at ? new Date(task.archived_at).toLocaleString('zh-CN') : '—'}
                  </span>
                  <IconButton
                    label={`恢复 ${task.config.name}`}
                    tooltip="恢复任务"
                    onClick={() => {
                      restore.reset()
                      setSelected(task)
                    }}
                  >
                    <RotateCcw />
                  </IconButton>
                </div>
              )
            })}
          </div>
        )}
        {query.data && (
          <Pagination
            page={page}
            total={query.data.total}
            pending={query.isFetching}
            onChange={setPage}
          />
        )}
      </DetailBody>
      <Modal
        open={!!selected}
        title="恢复任务"
        description={`恢复「${selected?.config.name ?? ''}」后保持暂停。`}
        onOpenChange={(v) => {
          if (!v && !restore.isPending) setSelected(null)
        }}
      >
        <div className="modal-body">
          {restore.error && (
            <p className="error" role="alert">
              {restore.error.message}
            </p>
          )}
        </div>
        <div className="modal-footer">
          <Button disabled={restore.isPending} onClick={() => setSelected(null)}>
            取消
          </Button>
          <Button
            variant="solid"
            disabled={restore.isPending}
            onClick={() => selected && restore.mutate(selected)}
          >
            {restore.isPending ? '正在恢复…' : '恢复'}
          </Button>
        </div>
      </Modal>
    </>
  )
}
