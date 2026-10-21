import { useContext, useEffect, useId, useRef, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { type User, workspaceId } from '../api'
import { t } from '../i18n'
import { useToast } from '../Toast'
import { Modal } from '../ui'
import { CopyValue } from '../runs/CopyValue'
import { OriginalImage } from './OriginalImage'
import { PreviewActivity } from './PreviewActivity'
import { executionId, readPreview, imageMimes, imageRead, type ImageSource } from './preview'
import './files.css'

export function ResultImage({
  href,
  alt,
  source,
  linked = false,
}: {
  href: string
  alt: string
  source?: ImageSource
  linked?: boolean
}) {
  const me = useQuery<User>({ queryKey: ['me'], enabled: false })
  const allowed = !!source && !!me.data && me.data.role !== 'viewer' && !me.data.workspace?.is_demo
  const client = useQueryClient(),
    toast = useToast()
  const root = useRef<HTMLSpanElement>(null)
  const [visible, setVisible] = useState(false),
    [open, setOpen] = useState(false),
    [broken, setBroken] = useState(false)
  const pin = useContext(PreviewActivity),
    pinId = useId()
  useEffect(() => {
    if (!allowed || !root.current) return
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          setVisible(true)
          observer.disconnect()
        }
      },
      { rootMargin: '100px 0px' },
    )
    observer.observe(root.current)
    return () => observer.disconnect()
  }, [allowed, href, source?.runId, source?.nodeId, source?.document, source?.revision])
  const queryKey = [
    'result-thumbnail',
    workspaceId,
    me.data?.id,
    source?.runId,
    source?.nodeId,
    source?.document,
    source?.revision,
    href,
  ]
  const query = useQuery({
    queryKey,
    enabled: allowed && visible,
    queryFn: ({ signal }) =>
      imageRead(signal, async () => {
        const id = await executionId(source!, signal)
        const value = await readPreview(id, href, signal, source?.document, 'thumbnail')
        if (value.format !== 'image' || !imageMimes.includes(value.mime) || value.truncated)
          throw new Error(t('filePreview.unsupportedImage'))
        return value
      }),
    staleTime: 60_000,
    gcTime: 60_000,
    retry: false,
    retryOnMount: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  })
  useEffect(() => {
    if (allowed && query.error) toast(query.error.message)
  }, [allowed, query.error, toast])
  useEffect(() => {
    if (!allowed) {
      setOpen(false)
      void client.cancelQueries({ queryKey, exact: true })
      client.removeQueries({ queryKey, exact: true })
    }
  }, [allowed, client, source?.runId, source?.nodeId, source?.document, source?.revision, href])
  useEffect(() => {
    setOpen(false)
    setBroken(false)
  }, [href, source?.runId, source?.nodeId, source?.document, source?.revision])
  useEffect(() => {
    pin(pinId, open)
    return () => pin(pinId, false)
  }, [pin, pinId, open])
  if (!allowed)
    return (
      <span title={href} data-result-image-href={href} data-result-image-label={alt || href}>
        {alt || href}
      </span>
    )
  const image = query.data
  const src = image && `data:${image.mime};base64,${image.content}`
  const failed = !!query.error || broken
  const picture = src && image && (
    <img
      src={src}
      alt={alt || image.name}
      onError={() => {
        setBroken(true)
        toast(t('filePreview.unsupportedImage'))
      }}
    />
  )
  return (
    <span
      className="result-inline-image"
      ref={root}
      data-result-image-href={href}
      data-result-image-label={alt || href}
      title={href}
    >
      {src && !failed ? (
        linked ? (
          picture
        ) : (
          <button
            type="button"
            className="result-image-open"
            onClick={() => setOpen(true)}
            aria-label={t('filePreview.enlargeImage', { name: alt || image.name })}
          >
            {picture}
          </button>
        )
      ) : (
        <span className="result-image-placeholder">
          <span>{alt || href}</span>
          {failed && (
            <span className="result-image-error">
              {t('filePreview.thumbnailFailed', {
                reason: query.error?.message ?? t('filePreview.unsupportedImage'),
              })}
            </span>
          )}
          {failed ? (
            !linked && (
              <button
                type="button"
                onClick={() => {
                  setBroken(false)
                  void query.refetch()
                }}
              >
                {t('重试')}
              </button>
            )
          ) : (
            <span className="quiet">{t('正在加载…')}</span>
          )}
        </span>
      )}
      {open && image && src && (
        <Modal
          open
          title={alt || image.name}
          className="file-preview-modal"
          onOpenChange={setOpen}
          actions={<CopyValue label={t('filePreview.copyPath')} value={image.path} toast={toast} />}
        >
          <div className="file-preview-content">
            <OriginalImage preview={image} href={href} source={source!} alt={alt || image.name} />
          </div>
        </Modal>
      )}
    </span>
  )
}
