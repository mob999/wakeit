import { useEffect, useRef } from 'react'
import { RotateCcw } from 'lucide-react'
import { terminal, type Run } from '../api'
import { IconButton } from '../ui'
import { CopyValue } from './CopyValue'
export function RunLogs({
  run,
  text,
  follow,
  visible,
  error,
  retry,
}: {
  run: Run
  text: string
  follow: boolean
  visible: boolean
  error: boolean
  retry: () => void
}) {
  const output = useRef<HTMLPreElement>(null)
  useEffect(() => {
    if (visible && follow && output.current) output.current.scrollTop = output.current.scrollHeight
  }, [text, visible, follow])
  return (
    <div className="log-panel">
      {error && (
        <div className="log-connection" role="status">
          <span>日志连接已中断，正在重连</span>
          <IconButton label="重新连接日志" onClick={retry}>
            <RotateCcw />
          </IconButton>
        </div>
      )}
      <pre ref={output} className="logs">
        {text || (terminal(run.state) || run.state === 'unknown' ? '暂无输出' : '等待执行输出…')}
        {['queued', 'starting', 'running'].includes(run.state) && <span className="cursor">▌</span>}
      </pre>
    </div>
  )
}

export function RunLogActions({
  text,
  follow,
  onFollow,
  toast,
}: {
  text: string
  follow: boolean
  onFollow: (follow: boolean) => void
  toast: (message: string) => void
}) {
  return (
    <>
      <label className="check">
        <input type="checkbox" checked={follow} onChange={(e) => onFollow(e.target.checked)} />
        跟随输出
      </label>
      <CopyValue value={text} toast={toast} />
    </>
  )
}
