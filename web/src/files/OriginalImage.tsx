import { useEffect, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { workspaceId } from '../api/client'
import { type User } from '../api/types'
import { t } from '../i18n'
import { useToast } from '../components/Toast'
import { executionId, readOriginal, type ImageSource, type Preview } from './preview'

// Mounted only inside an open preview. Original bytes and object URLs are
// discarded on close, permission loss, source change, and failed requests.
export function OriginalImage({
  preview,
  href,
  source,
  alt,
}: {
  preview: Preview
  href: string
  source: ImageSource
  alt: string
}) {
  const toast = useToast()
  const me = useQuery<User>({ queryKey: ['me'], enabled: false })
  const [resource, setResource] = useState<{ blob: Blob; url: string } | null>(null)
  const [broken, setBroken] = useState(false)
  const original = useQuery({
    queryKey: [
      'result-image-original',
      workspaceId,
      me.data?.id,
      source.runId,
      source.nodeId,
      source.document,
      source.revision,
      href,
      preview.revision,
    ],
    queryFn: async ({ signal }) =>
      readOriginal(await executionId(source, signal), href, signal, source.document),
    gcTime: 0,
    staleTime: Infinity,
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  })
  useEffect(() => {
    if (!original.data) {
      setResource(null)
      return
    }
    setBroken(false)
    const next = URL.createObjectURL(original.data)
    setResource({ blob: original.data, url: next })
    return () => URL.revokeObjectURL(next)
  }, [original.data])
  useEffect(() => {
    if (original.error) toast(original.error.message)
  }, [original.error, toast])
  const url = resource?.blob === original.data ? resource?.url : ''
  const error = original.error?.message ?? (broken ? t('filePreview.unsupportedImage') : '')
  return (
    <>
      {(original.isFetching || original.isPending || error) && (
        <div className="file-preview-image-status" role="status">
          <span>
            {error
              ? t('filePreview.originalFailed', { reason: error })
              : t('filePreview.loadingOriginal')}
          </span>
          {error && (
            <button
              type="button"
              onClick={() => {
                setBroken(false)
                void original.refetch()
              }}
            >
              {t('重试')}
            </button>
          )}
        </div>
      )}
      <img
        className="file-preview-image"
        src={
          url && !error && !original.isFetching
            ? url
            : `data:${preview.mime};base64,${preview.content}`
        }
        alt={alt}
        onError={() => {
          setBroken(true)
          toast(t('filePreview.unsupportedImage'))
        }}
      />
    </>
  )
}
