import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { IconButton } from '../ui'
import { ActionIcon } from '../actions/ActionIcon'
import { useCopy } from '../actions/useCopy'
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
  const { copied, copy } = useCopy(toast, label)
  return (
    <IconButton
      label={label}
      tooltip={copied ? tr('已复制') : label}
      onClick={() => void copy(value)}
    >
      <ActionIcon name={copied ? 'check' : 'copy'} />
    </IconButton>
  )
}
