import { useState } from 'react'
import { Copy, Check } from 'lucide-react'
import { IconButton } from '../ui'
export function CopyValue({
  value,
  toast,
  label = '复制',
}: {
  value: string
  toast: (message: string) => void
  label?: string
}) {
  const [copied, setCopied] = useState(false)
  return (
    <IconButton
      label={label}
      tooltip={copied ? '已复制' : label}
      onClick={() =>
        navigator.clipboard
          .writeText(value)
          .then(() => {
            setCopied(true)
            setTimeout(() => setCopied(false), 1500)
          })
          .catch(() => toast('请选中文本后复制'))
      }
    >
      {copied ? <Check size={14} /> : <Copy size={14} />}
    </IconButton>
  )
}
