import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { MessagesSquare } from 'lucide-react'
import { api, terminal, type Run, type Runner } from '../api'
import { Button, IconButton, Modal } from '../ui'
import { CopyValue } from '../runs/CopyValue'
import { ChoicePicker } from '../ChoicePicker'

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
  useTranslation()
  const client = useQueryClient()
  const [selected, setSelected] = useState<Run | null>(null)
  const [graph, setGraph] = useState<Run | null>(null)
  const [node, setNode] = useState('')
  const [loading, setLoading] = useState(false)
  const generation = useRef(0)
  const runners = useQuery<Runner[]>({
    queryKey: ['runners'],
    queryFn: () => api('/runners'),
    enabled: !!selected,
  })
  const device =
    runners.data?.find((r) => r.id === selected?.session?.runner_id)?.name ??
    selected?.config.runner
  if (!available || (run && !run.config.dag && !run.session?.command?.trim())) return null
  function close() {
    generation.current++
    setSelected(null)
    setGraph(null)
    setLoading(false)
  }
  async function choose(current: Run, nodeId: string) {
    const request = ++generation.current
    setNode(nodeId)
    setSelected(null)
    setLoading(true)
    try {
      const response = await api<{ execution: Run | null }>(
        `/runs/${id}/nodes/${encodeURIComponent(nodeId)}`,
      )
      if (request !== generation.current) return
      if (!response.execution?.session?.ready || !response.execution.session.command?.trim())
        throw new Error(tr('该会话暂不可续聊'))
      setSelected(response.execution)
    } catch (e) {
      if (request === generation.current) toast((e as Error).message)
    } finally {
      if (request === generation.current) setLoading(false)
    }
  }
  async function open() {
    if (loading) return
    const request = ++generation.current
    setLoading(true)
    try {
      const current =
        run ??
        (await client.fetchQuery<Run>({
          queryKey: ['run', id],
          queryFn: () => api(`/runs/${id}`),
          staleTime: 2000,
        }))
      if (request !== generation.current) return
      if (!terminal(current.state)) throw new Error(tr('该会话暂不可续聊'))
      if (current.config.dag) {
        const first = current.nodes?.find((n) => n.continuation_available)
        if (!first) throw new Error(tr('该会话暂不可续聊'))
        setGraph(current)
        await choose(current, first.id)
      } else {
        if (!current.session?.ready || !current.session.command?.trim())
          throw new Error(current.session?.reason || tr('该会话暂不可续聊'))
        setSelected(current)
      }
    } catch (e) {
      if (request === generation.current) toast((e as Error).message)
    } finally {
      if (request === generation.current) setLoading(false)
    }
  }
  return (
    <>
      <IconButton label={tr('续聊')} loading={loading && !graph} onClick={open}>
        <MessagesSquare />
      </IconButton>
      {(selected || graph) && (
        <Modal open title={tr('续聊')} onOpenChange={(open) => !open && close()}>
          <div className="modal-body">
            {graph && (
              <ChoicePicker
                label={tr('dag.selectNode')}
                value={node}
                options={(graph.nodes ?? [])
                  .filter((n) => n.continuation_available)
                  .map((n) => ({ value: n.id, label: n.name }))}
                onChange={(id) => choose(graph, id)}
              />
            )}
            {loading && <p className="quiet">{tr('正在加载…')}</p>}
            {selected?.session && (
              <>
                <dl className="properties">
                  <dt>{tr('执行设备')}</dt>
                  <dd>{device}</dd>
                  <dt>{tr('工作目录')}</dt>
                  <dd className="mono wrap">{selected.session.cwd}</dd>
                </dl>
                <div className="session-resume-command">
                  <code>{selected.session.command}</code>
                  <CopyValue
                    label={tr('复制续聊命令')}
                    value={selected.session.command!}
                    toast={toast}
                  />
                </div>
                <p className="quiet">{tr('在 {{v0}} 上执行。', { v0: device })}</p>
              </>
            )}
          </div>
          <div className="modal-footer">
            <Button onClick={close}>{tr('关闭')}</Button>
          </div>
        </Modal>
      )}
    </>
  )
}
