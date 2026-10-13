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
  return (
    <div className="settings-request content-card" role={error ? 'alert' : 'status'}>
      {error ? (
        <>
          <span>{error.message}</span>
          <Button onClick={retry}>
            <RefreshCw size={14} />
            重试
          </Button>
        </>
      ) : pending ? (
        '正在加载…'
      ) : null}
    </div>
  )
}
export function SecretValue({ label, value }: { label: string; value: string }) {
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
          label={`复制${label}`}
          tooltip={copied ? '已复制' : `复制${label}`}
          onClick={async () => {
            try {
              await navigator.clipboard.writeText(value)
              setCopied(true)
              setTimeout(() => setCopied(false), 1500)
              setError('')
            } catch {
              setError('复制失败，请手动复制')
            }
          }}
        >
          {copied ? <Check /> : <Copy />}
        </IconButton>
      </div>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </>
  )
}
export const date = (value: string) => new Date(value).toLocaleDateString('zh-CN')
