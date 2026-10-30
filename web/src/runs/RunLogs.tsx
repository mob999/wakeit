import { ActionIcon } from '../actions/ActionIcon'
import { ErrorToast } from '../components/Toast'
import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useEffect, useRef } from 'react'

import { terminal } from './state'
import { type Run } from '../api/types'
import { IconButton } from '../components/ui'
import { CopyValue } from './CopyValue'
export function RunLogs({
  run,
  text,
  follow,
  onFollowChange,
  visible,
  error,
  retry,
}: {
  run: Run
  text: string
  follow: boolean
  onFollowChange: (follow: boolean) => void
  visible: boolean
  error: boolean
  retry: () => void
}) {
  useTranslation()
  const output = useRef<HTMLPreElement>(null)
  useEffect(() => {
    if (visible && follow && output.current) output.current.scrollTop = output.current.scrollHeight
  }, [text, visible, follow])
  return (
    <div className="log-panel">
      {error && (
        <div className="log-connection" role="status">
          <ErrorToast error={tr('日志连接已中断，正在重连')} />
          <IconButton label={tr('重新连接日志')} onClick={retry}>
            <ActionIcon name="refresh" />
          </IconButton>
        </div>
      )}
      <pre
        ref={output}
        className="logs"
        onWheel={(event) => {
          if (event.deltaY < 0 && follow) onFollowChange(false)
        }}
        onTouchStart={() => {
          if (follow) onFollowChange(false)
        }}
        onScroll={(event) => {
          const pane = event.currentTarget
          if (follow && pane.scrollHeight - pane.clientHeight - pane.scrollTop > 48)
            onFollowChange(false)
        }}
      >
        {text ||
          (terminal(run.state) || run.state === 'unknown' ? tr('暂无输出') : tr('等待执行输出…'))}
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
  useTranslation()
  return (
    <>
      <label className="check">
        <input type="checkbox" checked={follow} onChange={(e) => onFollow(e.target.checked)} />
        {tr('跟随输出')}
      </label>
      <CopyValue label={tr('actions.copyLogs')} value={text} toast={toast} />
    </>
  )
}
