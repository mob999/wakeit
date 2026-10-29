import { useEffect, useRef, useState, useContext, useId, type ReactNode } from 'react'
import { useQuery } from '@tanstack/react-query'
import { type User, type Runner } from '../api/types'
import { t } from '../i18n'
import { useToast } from '../components/Toast'
import { Modal, Button } from '../components/ui'
import { CopyValue } from '../runs/CopyValue'
import { MarkdownContent } from '../markdown/MarkdownContent'
import type { FileSource } from './links'
import './files.css'
import { OriginalImage } from './OriginalImage'
import { PreviewActivity } from './PreviewActivity'
import { type Preview, executionId, readPreview, imageMimes } from './preview'
export function ResultFileLink({
  href,
  source,
  children,
  accessibleLabel,
}: {
  href: string
  source?: FileSource
  children: ReactNode
  accessibleLabel?: string
}) {
  const toast = useToast()
  const me = useQuery<User>({ queryKey: ['me'], enabled: false })
  const runners = useQuery<Runner[]>({ queryKey: ['runners'], enabled: false })
  const allowed = !!source && !!me.data && me.data.role !== 'viewer' && !me.data.workspace?.is_demo
  const [open, setOpen] = useState(false),
    [actualId, setActualId] = useState(''),
    [documentVersion, setDocumentVersion] = useState(''),
    [value, setValue] = useState<Preview | null>(null),
    [pending, setPending] = useState(false),
    [failed, setFailed] = useState('')
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
    setFailed('')
    try {
      const id = await executionId(source, controller.signal)
      const result = await readPreview(id, href, controller.signal, undefined, 'thumbnail')
      const revision =
        result.format === 'markdown'
          ? Array.from(
              new Uint8Array(
                await crypto.subtle.digest('SHA-256', new TextEncoder().encode(result.content)),
              ),
              (byte) => byte.toString(16).padStart(2, '0'),
            ).join('')
          : ''
      if (!controller.signal.aborted && writable.current) {
        setActualId(id)
        setDocumentVersion(revision)
        setValue(result)
      }
    } catch (error) {
      if (!controller.signal.aborted) {
        setFailed((error as Error).message)
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
        aria-label={accessibleLabel}
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
              <p className="quiet text-muted">{t('正在加载…')}</p>
            ) : value ? (
              <>
                <div className="file-preview-meta" title={value.modified_at ?? undefined}>
                  <span>{value.size.toLocaleString()} B</span>
                  {value.truncated && <span>{t('结果已截断')}</span>}
                </div>
                {value.format === 'image' && imageMimes.includes(value.mime) ? (
                  <OriginalImage
                    preview={value}
                    href={href}
                    source={{ runId: actualId }}
                    alt={value.name}
                  />
                ) : value.format === 'markdown' ? (
                  <article className="markdown-result">
                    <MarkdownContent
                      text={value.content}
                      imageSource={{
                        runId: actualId,
                        document: href,
                        revision: documentVersion,
                      }}
                    />
                  </article>
                ) : (
                  <pre className="code">{text}</pre>
                )}
                {!value.content && <p className="quiet text-muted">{t('filePreview.empty')}</p>}
              </>
            ) : failed ? (
              <div className="file-preview-image-status" role="status">
                <span>{failed}</span>
                <Button onClick={() => void load()}>{t('重试')}</Button>
              </div>
            ) : null}
          </div>
        </Modal>
      )}
    </>
  )
}
