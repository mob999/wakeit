import { ActionIcon } from '../actions/ActionIcon'
import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useRef, useState, useEffect } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'

import { api, terminal, type Run, type Runner, type User } from '../api'
import { IconButton, Modal } from '../ui'
import { CopyValue } from '../runs/CopyValue'
import { ChoicePicker } from '../ChoicePicker'
import { TerminalSession, type TerminalHandle } from './TerminalSession'

export function ResumeRun({
  run,
  id,
  available,
  toast,
  parentId,
  initialNode,
  strictNode = false,
  onActiveChange,
}: {
  run?: Run
  parentId?: string
  initialNode?: string
  strictNode?: boolean
  onActiveChange?: (active: boolean) => void
  id: string
  available: boolean
  toast: (message: string) => void
}) {
  useTranslation()
  const client = useQueryClient()
  const [selected, setSelected] = useState<Run | null>(null)
  const [graph, setGraph] = useState<Run | null>(null)
  const [node, setNode] = useState('')
  useEffect(() => {
    onActiveChange?.(!!selected || !!graph)
    return () => onActiveChange?.(false)
  }, [selected, graph, onActiveChange])
  const [loading, setLoading] = useState(false)
  const generation = useRef(0)
  const terminalRef = useRef<TerminalHandle>(null)
  const trigger = useRef<HTMLButtonElement>(null)
  const [closing, setClosing] = useState(false)
  const [status, setStatus] = useState('connecting')
  const me = useQuery<User>({ queryKey: ['me'], enabled: false })
  const runners = useQuery<Runner[]>({
    queryKey: ['runners'],
    queryFn: () => api('/runners'),
    enabled: !!selected,
  })
  const device =
    runners.data?.find((r) => r.id === selected?.session?.runner_id)?.name ??
    selected?.config.runner
  if (!available || (run && !run.config.dag && !run.session?.command?.trim())) return null
  async function close() {
    if (closing) return
    setClosing(true)
    await terminalRef.current?.close()
    setClosing(false)
    generation.current++
    setSelected(null)
    setGraph(null)
    setLoading(false)
  }
  async function choose(nodeId: string) {
    if (closing) return
    setClosing(true)
    const stopped = await terminalRef.current?.close()
    setClosing(false)
    if (stopped === false) return
    const request = ++generation.current
    setNode(nodeId)
    setSelected(null)
    setLoading(true)
    try {
      const response = await api<{ execution: Run | null }>(
        `/runs/${parentId ?? id}/nodes/${encodeURIComponent(nodeId)}`,
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
      const source = parentId ?? id
      const current =
        (!parentId && run) ||
        (await client.fetchQuery<Run>({
          queryKey: ['run', source],
          queryFn: () => api(`/runs/${source}`),
          staleTime: 2000,
        }))
      if (request !== generation.current) return
      if (current.config.dag) {
        const first =
          current.nodes?.find((n) => n.id === initialNode && n.continuation_available) ??
          (!strictNode ? current.nodes?.find((n) => n.continuation_available) : undefined)
        if (!first) throw new Error(tr('该会话暂不可续聊'))
        setGraph(current)
        await choose(first.id)
      } else {
        if (!terminal(current.state)) throw new Error(tr('该会话暂不可续聊'))
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
      <IconButton ref={trigger} label={tr('续聊')} loading={loading && !graph} onClick={open}>
        <ActionIcon name="resumeSession" />
      </IconButton>
      {(selected || graph) && (
        <Modal
          open
          title={tr('续聊')}
          hideTitle
          terminal
          closePending={closing}
          className="terminal-modal"
          onOpenChange={(open) => {
            if (!open) void close()
          }}
          onOpenAutoFocus={(event) => event.preventDefault()}
          onCloseAutoFocus={(event) => {
            event.preventDefault()
            trigger.current?.focus()
          }}
          actions={
            <>
              <div className="terminal-context">
                {graph &&
                  (graph.nodes?.filter((n) => n.continuation_available).length ?? 0) > 1 && (
                    <ChoicePicker
                      label={tr('dag.selectNode')}
                      value={node}
                      searchable
                      disabled={closing || loading}
                      options={(graph.nodes ?? [])
                        .filter((n) => n.continuation_available)
                        .map((n) => ({ value: n.id, label: n.name, description: n.id }))}
                      onChange={(id) => void choose(id)}
                    />
                  )}
                <span
                  className="terminal-device"
                  title={`${device ?? ''} / ${selected?.session?.agent ?? ''}`}
                >
                  {device} / {selected?.session?.agent}
                </span>
                <span className="terminal-connection" role="status">
                  {tr(`terminal.${closing ? 'closing' : loading ? 'connecting' : status}`)}
                </span>
              </div>
              {selected?.session?.command && (
                <CopyValue
                  label={tr('复制续聊命令')}
                  value={selected.session.command}
                  toast={toast}
                />
              )}
            </>
          }
        >
          {selected?.session?.command && (
            <TerminalSession
              key={selected.id}
              ref={terminalRef}
              runId={selected.id}
              command={selected.session.command}
              device={device ?? selected.config.runner}
              writable={!!me.data && me.data.role !== 'viewer' && !me.data.workspace?.is_demo}
              toast={toast}
              onState={setStatus}
            />
          )}
        </Modal>
      )}
    </>
  )
}
