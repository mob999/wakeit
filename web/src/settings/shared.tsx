import { useCopy } from '../actions/useCopy'
import { ActionIcon } from '../actions/ActionIcon'
import { ErrorToast } from '../components/Toast'
import { t as tr, locale } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useState } from 'react'
import { RefreshCw } from '../components/icons'
import { Button, IconButton } from '../components/ui'
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
  const [error, setError] = useState('')
  const { copied, copy } = useCopy(
    () => setError(tr('复制失败，请手动复制')),
    tr('复制{{v0}}', { v0: label }),
  )
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
          onClick={() => {
            setError('')
            void copy(value)
          }}
        >
          {copied ? <ActionIcon name="check" /> : <ActionIcon name="copy" />}
        </IconButton>
      </div>
      {error && <ErrorToast error={error} />}
    </>
  )
}
export const date = (value: string) => new Date(value).toLocaleDateString(locale())
