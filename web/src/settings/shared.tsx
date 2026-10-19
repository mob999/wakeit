import { ErrorToast } from '../Toast'
import { t as tr, locale } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useState } from 'react'
import { Check, Copy, RefreshCw } from 'lucide-react'
import { Button, IconButton } from '../ui'
export function RequestState({
  pending,
  error,
  retry,
}: {
  pending: boolean
  error: Error | null
  retry: () => void
}) {
  useTranslation()
  if (error)
    return (
      <>
        <ErrorToast error={error} />
        <div className="settings-request content-card">
          <Button onClick={retry}>
            <RefreshCw size={14} />
            {tr('重试')}
          </Button>
        </div>
      </>
    )
  return (
    <div className="settings-request content-card" role="status">
      {pending ? tr('正在加载…') : null}
    </div>
  )
}
export function SecretValue({ label, value }: { label: string; value: string }) {
  useTranslation()
  const [copied, setCopied] = useState(false)
  const [error, setError] = useState('')
  return (
    <>
      <div className="secret-result">
        <input
          aria-label={label}
          className="mono"
          value={value}
          readOnly
          onFocus={(e) => e.target.select()}
        />
        <IconButton
          label={tr('复制{{v0}}', { v0: label })}
          tooltip={copied ? tr('已复制') : tr('复制{{v0}}', { v0: label })}
          onClick={async () => {
            try {
              await navigator.clipboard.writeText(value)
              setCopied(true)
              setTimeout(() => setCopied(false), 1500)
              setError('')
            } catch {
              setError(tr('复制失败，请手动复制'))
            }
          }}
        >
          {copied ? <Check /> : <Copy />}
        </IconButton>
      </div>
      {error && <ErrorToast>{error}</ErrorToast>}
    </>
  )
}
export const date = (value: string) => new Date(value).toLocaleDateString(locale())
