import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import { api, apiUrl, ApiError } from '../api'
import { t as tr } from '../i18n'
import './terminal.css'

type Record = { id: string; state: string; reason?: string }
export type TerminalHandle = { close: () => Promise<boolean> }
const bytes = (value: string) => Uint8Array.from(atob(value), (c) => c.charCodeAt(0))
const encoded = (value: string) => btoa(String.fromCharCode(...new TextEncoder().encode(value)))
const displayText = (value: string) => value.replace(/[\x00-\x1f\x7f-\x9f]/g, ' ')

export const TerminalSession = forwardRef<
  TerminalHandle,
  {
    runId: string
    command: string
    device: string
    writable: boolean
    toast: (text: string) => void
    onState: (state: string) => void
  }
>(function TerminalSession(props, ref) {
  const host = useRef<HTMLDivElement>(null)
  const current = useRef(props)
  current.current = props
  const shutdown = useRef<() => Promise<boolean>>(async () => true)
  const [status, setStatus] = useState('connecting')
  useImperativeHandle(ref, () => ({ close: () => shutdown.current() }), [])
  useEffect(() => {
    const term = new Terminal({
      fontSize: 12,
      fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
      scrollback: 2000,
      cursorBlink: true,
      allowProposedApi: false,
      disableStdin: true,
      convertEol: false,
    })
    const fit = new FitAddon()
    term.loadAddon(fit)
    term.open(host.current!)
    function theme() {
      const style = getComputedStyle(host.current!)
      term.options.theme = {
        background: style.getPropertyValue('--canvas').trim(),
        foreground: style.getPropertyValue('--ink').trim(),
        cursor: style.getPropertyValue('--ink').trim(),
        selectionBackground: style.getPropertyValue('--muted').trim() + '55',
      }
    }
    theme()
    const observer = new MutationObserver(theme)
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ['class', 'data-theme'],
    })
    term.writeln(displayText(props.device))
    term.writeln('$ ' + displayText(props.command))
    term.writeln('')
    let record: Record | undefined
    let ws: WebSocket | undefined
    let cancelled = false
    let leaving = false
    let ended = false
    let ready = false
    let retry: ReturnType<typeof setTimeout> | undefined
    let heartbeat: ReturnType<typeof setInterval> | undefined
    let lostAt = 0
    let inputSeq = 0
    let ack = true
    let pending = ''
    let connecting = false
    let boot: Promise<void> = Promise.resolve()
    function state(next: string) {
      if (cancelled) return
      setStatus(next)
      current.current.onState(next)
    }
    function send(kind: string, data?: unknown) {
      if (ws?.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ kind, data }))
    }
    function flush() {
      if (!ready || !ack || !pending) return
      let end = Math.min(4096, pending.length)
      if (end < pending.length && /[\uD800-\uDBFF]/.test(pending[end - 1])) end--
      const text = pending.slice(0, end)
      pending = pending.slice(text.length)
      ack = false
      send('input', { seq: ++inputSeq, data: encoded(text) })
    }
    const input = term.onData((data) => {
      if (!ready || cancelled) return
      if (pending.length + data.length > 32768) {
        current.current.toast(tr('terminal.inputBusy'))
        return
      }
      pending += data
      flush()
    })
    const size = new ResizeObserver(() => {
      try {
        fit.fit()
      } catch {
        return
      }
      if (ready) send('resize', { cols: term.cols, rows: term.rows })
    })
    size.observe(host.current!)
    function processState(value: { state?: string; reason?: string }) {
      if (value.reason) current.current.toast(tr(value.reason))
      if (value.state === 'running') {
        ready = true
        term.options.disableStdin = false
        state('connected')
      } else if (value.state === 'closed' || value.state === 'unknown') {
        ended = true
        ready = false
        term.options.disableStdin = true
        state(value.state === 'closed' ? 'ended' : 'pending')
      } else if (value.state === 'closing') {
        ready = false
        term.options.disableStdin = true
        state('closing')
      }
    }
    async function connect() {
      if (cancelled || ended || connecting || !record) return
      connecting = true
      try {
        const access = await api<{ ticket: string }>(
          `/terminals/${record.id}/attach`,
          'POST',
          undefined,
          undefined,
          AbortSignal.timeout(5000),
        )
        if (cancelled) return
        const url = new URL(apiUrl(`/terminals/${record.id}/stream`), location.href)
        url.protocol = location.protocol === 'https:' ? 'wss:' : 'ws:'
        inputSeq = 0
        ack = true
        const socket = new WebSocket(url)
        ws = socket
        socket.onopen = () => {
          if (cancelled) {
            socket.close()
            return
          }
          try {
            fit.fit()
          } catch {}
          socket.send(JSON.stringify({ ticket: access.ticket, cols: term.cols, rows: term.rows }))
          heartbeat = setInterval(() => send('heartbeat'), 5000)
        }
        socket.onmessage = (event) => {
          if (cancelled || ws !== socket) return
          try {
            const frame = JSON.parse(event.data)
            if (frame.kind === 'snapshot') {
              if (frame.data.screen) {
                term.reset()
                term.write(bytes(frame.data.screen))
              }
              lostAt = 0
              processState(frame.data.state)
            } else if (frame.kind === 'output') term.write(bytes(frame.data.data))
            else if (frame.kind === 'state') processState(frame.data)
            else if (frame.kind === 'ack') {
              ack = true
              flush()
            } else if (frame.kind === 'error') {
              current.current.toast(tr(frame.data.message))
              ended = true
              ready = false
              term.options.disableStdin = true
              state('ended')
            }
          } catch {
            current.current.toast(tr('terminal.invalidResponse'))
            ended = true
            socket.close()
          }
        }
        socket.onclose = () => {
          if (ws !== socket) return
          if (heartbeat) clearInterval(heartbeat)
          ready = false
          pending = ''
          ack = true
          term.options.disableStdin = true
          if (!cancelled && !ended) reconnect()
        }
      } catch (error) {
        if (cancelled) return
        if (error instanceof ApiError && [401, 403, 404].includes(error.status)) {
          current.current.toast(error.message)
          ended = true
          state('ended')
        } else reconnect()
      } finally {
        connecting = false
      }
    }
    function reconnect() {
      if (!lostAt) lostAt = Date.now()
      if (Date.now() - lostAt >= 30000) {
        ended = true
        state('ended')
        current.current.toast(tr('terminal.disconnected'))
        return
      }
      state('reconnecting')
      retry = setTimeout(() => void connect(), 1000)
    }
    async function close() {
      if (cancelled) return false
      cancelled = true
      ready = false
      term.options.disableStdin = true
      if (retry) clearTimeout(retry)
      if (heartbeat) clearInterval(heartbeat)
      await boot
      let confirmed = true
      try {
        if (record) {
          const stopped = await api<Record>(
            `/terminals/${record.id}/close`,
            'POST',
            undefined,
            undefined,
            AbortSignal.timeout(12000),
          )
          confirmed = stopped.state === 'closed'
          if (!confirmed) current.current.toast(tr('terminal.cleanupPending'))
        }
      } catch (error) {
        confirmed = false
        current.current.toast((error as Error).message)
      } finally {
        ws?.close()
      }
      return confirmed
    }
    shutdown.current = close
    const start = setTimeout(() => {
      if (!props.writable) {
        state('copyOnly')
        return
      }
      boot = (async () => {
        try {
          record = await api<Record>(
            `/runs/${props.runId}/terminals`,
            'POST',
            {
              request_id: crypto.randomUUID(),
            },
            undefined,
            AbortSignal.timeout(5000),
          )
          if (!cancelled) await connect()
        } catch (error) {
          if (!cancelled) {
            current.current.toast((error as Error).message)
            state('copyOnly')
          }
        }
      })()
    }, 0)
    const pagehide = () => {
      leaving = true
    }
    const navigation = (event: MouseEvent) => {
      const link = (event.target as Element)?.closest('a')
      if (
        link &&
        new URL(link.href, location.href).pathname.match(/^\/w\//) &&
        !link.pathname.startsWith(location.pathname.match(/^\/w\/[^/]+/)?.[0] ?? '')
      )
        void close()
    }
    window.addEventListener('pagehide', pagehide)
    document.addEventListener('click', navigation, true)
    term.focus()
    return () => {
      clearTimeout(start)
      if (!leaving) void close()
      else {
        cancelled = true
        ws?.close()
      }
      if (retry) clearTimeout(retry)
      if (heartbeat) clearInterval(heartbeat)
      window.removeEventListener('pagehide', pagehide)
      document.removeEventListener('click', navigation, true)
      size.disconnect()
      observer.disconnect()
      input.dispose()
      term.dispose()
    }
  }, [props.runId, props.writable])
  return (
    <div
      ref={host}
      className="session-terminal"
      aria-label={tr('terminal.screen')}
      data-status={status}
    />
  )
})
