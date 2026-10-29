import { t as tr } from '../i18n'
import { CreateTaskDialog } from '../tasks/CreateTaskDialog'
import { DagEditor } from './DagEditor'
import { useQuery } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { api } from '../api/client'
import { type User, type Task } from '../api/types'
import { Empty } from '../components/Empty'

export function DagScreen({ user, search }: { user: User; search: { task?: string } }) {
  const navigate = useNavigate()
  const settings = useQuery<{ timezone: string }>({
    queryKey: ['settings'],
    queryFn: () => api('/settings'),
  })
  const task = useQuery<Task>({
    queryKey: ['tasks', 'detail', search.task],
    queryFn: () => api(`/tasks/${encodeURIComponent(search.task!)}`),
    enabled: !!search.task,
  })
  if (user.role === 'viewer' || user.workspace?.is_demo)
    return <Empty title={tr('无法访问此工作区')} />
  if (settings.error || task.error)
    return <Empty title={tr('加载失败')} detail={(settings.error || task.error)?.message} />
  if (!settings.data || (search.task && !task.data)) return <Empty title={tr('正在加载…')} />
  if (task.data && (!task.data.config.dag || task.data.archived_at))
    return <Empty title={tr('dag.fixErrors')} />
  if (!search.task)
    return (
      <CreateTaskDialog
        initialTab="dag"
        timezone={settings.data.timezone}
        onClose={() => navigate({ to: '/tasks', search: {} })}
        onCreated={(id, runId) =>
          runId
            ? navigate({ to: '/runs', search: { run: runId } })
            : navigate({ to: '/tasks', search: { task: id } })
        }
      />
    )
  return (
    <DagEditor
      key={search.task ?? 'new'}
      task={task.data}
      timezone={settings.data.timezone}
      onClose={() => navigate({ to: '/tasks', search: { task: search.task } })}
      onSaved={(id) => navigate({ to: '/tasks', search: { task: id } })}
    />
  )
}
