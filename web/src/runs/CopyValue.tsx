import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useState } from 'react'
import { Copy, Check } from 'lucide-react'
import { IconButton } from '../ui'
export function CopyValue({
  value,
  toast,
  label = tr('复制'),
}: {
  value: string
  toast: (message: string) => void
  label?: string
}) {
  useTranslation()
  const [copied, setCopied] = useState(false)
  return (
    <IconButton
      label={label}
      tooltip={copied ? tr('已复制') : label}
      onClick={() =>
        navigator.clipboard
          .writeText(value)
          .then(() => {
            setCopied(true)
            setTimeout(() => setCopied(false), 1500)
          })
          .catch(() => toast(tr('请选中文本后复制')))
      }
    >
      {copied ? <Check size={14} /> : <Copy size={14} />}
    </IconButton>
  )
}
