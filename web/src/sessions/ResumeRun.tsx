import { useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { MessagesSquare } from 'lucide-react'
import { api, terminal, type Run, type Runner } from '../api'
import { Button, IconButton, Modal } from '../ui'
import { CopyValue } from '../runs/CopyValue'

export function ResumeRun({
  run,
  id,
  available,
  toast,
}: {
  run?: Run
  id: string
  available: boolean
  toast: (message: string) => void
}) {
  const client = useQueryClient()
  const [selected, setSelected] = useState<Run | null>(null)
  const [loading, setLoading] = useState(false)
  const runners = useQuery<Runner[]>({
    queryKey: ['runners'],
    queryFn: () => api('/runners'),
    enabled: !!selected,
  })
  const device =
    runners.data?.find((runner) => runner.id === selected?.session?.runner_id)?.name ??
    selected?.config.runner
  if (!available || (run && !run.session?.command?.trim())) return null
  async function open() {
    if (loading) return
    setLoading(true)
    try {
      const current =
        run ??
        (await client.fetchQuery<Run>({
          queryKey: ['run', id],
          queryFn: () => api<Run>(`/runs/${id}`),
          staleTime: 2000,
        }))
      if (!terminal(current.state) || !current.session?.ready || !current.session.command?.trim())
        throw new Error(current.session?.reason || '该会话暂不可续聊')
      setSelected(current)
    } catch (e) {
      toast((e as Error).message)
    } finally {
      setLoading(false)
    }
  }
  return (
    <>
      <IconButton label="续聊" loading={loading} onClick={open}>
        <MessagesSquare />
      </IconButton>
      {selected?.session && (
        <Modal open title="续聊" onOpenChange={(open) => !open && setSelected(null)}>
          <div className="modal-body">
            <dl className="properties">
              <dt>执行设备</dt>
              <dd>{device}</dd>
              <dt>工作目录</dt>
              <dd className="mono wrap">{selected.session.cwd}</dd>
            </dl>
            <div className="session-resume-command">
              <code>{selected.session.command}</code>
              <CopyValue label="复制续聊命令" value={selected.session.command!} toast={toast} />
            </div>
            <p className="quiet">在 {device} 上执行。</p>
          </div>
          <div className="modal-footer">
            <Button onClick={() => setSelected(null)}>关闭</Button>
          </div>
        </Modal>
      )}
    </>
  )
}
