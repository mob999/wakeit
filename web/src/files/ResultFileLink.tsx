import { useEffect, useRef, useState, useContext, useId, type ReactNode } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api, type User, type Runner, type Run } from '../api'
import { t } from '../i18n'
import { useToast } from '../Toast'
import { Modal, Button } from '../ui'
import { CopyValue } from '../runs/CopyValue'
import { MarkdownContent } from '../runs/MarkdownContent'
import type { FileSource } from './links'
import './files.css'
import { PreviewActivity } from './PreviewActivity'
type Preview = {
  path: string
  name: string
  runner_id: string
  format: 'text' | 'markdown' | 'json' | 'image'
  mime: string
  content: string
  size: number
  modified_at: string | null
  truncated: boolean
}
export function ResultFileLink({
  href,
  source,
  children,
}: {
  href: string
  source?: FileSource
  children: ReactNode
}) {
  const toast = useToast()
  const me = useQuery<User>({ queryKey: ['me'], enabled: false })
  const runners = useQuery<Runner[]>({ queryKey: ['runners'], enabled: false })
  const allowed = !!source && !!me.data && me.data.role !== 'viewer' && !me.data.workspace?.is_demo
  const [open, setOpen] = useState(false),
    [value, setValue] = useState<Preview | null>(null),
    [pending, setPending] = useState(false),
    [failed, setFailed] = useState(false)
  const previewActivity = useContext(PreviewActivity),
    previewId = useId()
  useEffect(() => {
    previewActivity(previewId, open)
    return () => previewActivity(previewId, false)
  }, [open, previewActivity, previewId])
  const request = useRef<AbortController | null>(null)
  const writable = useRef(allowed)
  writable.current = allowed
  useEffect(() => () => request.current?.abort(), [])
  useEffect(() => {
    request.current?.abort()
    setValue(null)
    setOpen(false)
    setPending(false)
  }, [source?.runId, source?.nodeId, href])
  useEffect(() => {
    if (!allowed) {
      request.current?.abort()
      setValue(null)
      setOpen(false)
    }
  }, [allowed])
  function close() {
    request.current?.abort()
    setOpen(false)
    setValue(null)
    setPending(false)
  }
  async function load() {
    if (!source || !writable.current) return
    request.current?.abort()
    const controller = new AbortController()
    request.current = controller
    setOpen(true)
    setPending(true)
    setFailed(false)
    try {
      let id = source.runId
      if (source.nodeId) {
        const detail = await api<{ execution: Run | null }>(
          `/runs/${id}/nodes/${encodeURIComponent(source.nodeId)}`,
          'GET',
          undefined,
          undefined,
          controller.signal,
        )
        if (!detail.execution) throw new Error(t('filePreview.noSource'))
        id = detail.execution.id
      }
      const result = await api<Preview>(
        `/runs/${id}/files/preview`,
        'POST',
        { href },
        undefined,
        controller.signal,
      )
      if (!controller.signal.aborted && writable.current) setValue(result)
    } catch (error) {
      if (!controller.signal.aborted) {
        setFailed(true)
        toast((error as Error).message)
      }
    } finally {
      if (!controller.signal.aborted) setPending(false)
    }
  }
  function copy() {
    navigator.clipboard.writeText(href).catch(() => toast(t('请选中文本后复制')))
  }
  let name = href.split(/[\\/]/).at(-1) || href
  try {
    name = decodeURIComponent(name)
  } catch {}
  let text = value?.content ?? ''
  if (value?.format === 'json') {
    try {
      text = JSON.stringify(JSON.parse(text), null, 2)
    } catch {}
  }
  const device = runners.data?.find((r) => r.id === value?.runner_id)?.name
  return (
    <>
      <button
        type="button"
        className="result-file-link"
        data-result-file-href={href}
        title={allowed ? href : `${t('filePreview.copyPath')}: ${href}`}
        onClick={allowed ? () => void load() : copy}
      >
        {children}
      </button>
      {open && (
        <Modal
          open
          title={value?.name ?? name}
          className="file-preview-modal"
          onOpenChange={(next) => {
            if (!next) close()
          }}
          actions={
            <>
              {device && (
                <span className="file-preview-device" title={value?.path ?? href}>
                  {device}
                </span>
              )}
              <CopyValue
                label={t('filePreview.copyPath')}
                value={value?.path ?? href}
                toast={toast}
              />
            </>
          }
        >
          <div className="file-preview-content">
            {pending ? (
              <p className="quiet">{t('正在加载…')}</p>
            ) : value ? (
              <>
                <div className="file-preview-meta" title={value.modified_at ?? undefined}>
                  <span>{value.size.toLocaleString()} B</span>
                  {value.truncated && <span>{t('结果已截断')}</span>}
                </div>
                {value.format === 'image' &&
                ['image/png', 'image/jpeg', 'image/webp'].includes(value.mime) ? (
                  <img
                    className="file-preview-image"
                    src={`data:${value.mime};base64,${value.content}`}
                    alt={value.name}
                  />
                ) : value.format === 'markdown' ? (
                  <article className="markdown-result">
                    <MarkdownContent text={value.content} />
                  </article>
                ) : (
                  <pre className="code">{text}</pre>
                )}
                {!value.content && <p className="quiet">{t('filePreview.empty')}</p>}
              </>
            ) : failed ? (
              <Button onClick={() => void load()}>{t('重试')}</Button>
            ) : null}
          </div>
        </Modal>
      )}
    </>
  )
}
