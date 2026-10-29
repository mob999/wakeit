import { useContext, useEffect, useId, useState, type ReactNode } from 'react'
import * as Dropdown from '@radix-ui/react-dropdown-menu'
import { IconButton } from '../components/ui'
import { t } from '../i18n'
import { useTranslation } from 'react-i18next'
import { PreviewActivity } from '../files/PreviewActivity'
import { ActionIcon, type ActionIconName } from './ActionIcon'
import { copyAction } from './copy'

export function ActionMenu({
  children,
  label = t('actions.more'),
  icon = 'more',
  loading = false,
}: {
  children: ReactNode
  label?: string
  icon?: ActionIconName
  loading?: boolean
}) {
  useTranslation()
  const [open, setOpen] = useState(false)
  const pin = useContext(PreviewActivity),
    id = useId()
  useEffect(() => {
    pin(id, open)
    return () => pin(id, false)
  }, [id, pin, open])
  return (
    <Dropdown.Root open={open} onOpenChange={setOpen}>
      <Dropdown.Trigger asChild>
        <IconButton label={label} loading={loading}>
          <ActionIcon name={icon} />
        </IconButton>
      </Dropdown.Trigger>
      <Dropdown.Portal>
        <Dropdown.Content
          className="dropdown action-menu"
          align="end"
          sideOffset={6}
          collisionPadding={12}
        >
          {children}
        </Dropdown.Content>
      </Dropdown.Portal>
    </Dropdown.Root>
  )
}
export function ActionMenuItem({
  icon,
  children,
  onSelect,
  disabled = false,
  reason,
  danger = false,
}: {
  icon: ActionIconName
  children: ReactNode
  onSelect: () => void
  disabled?: boolean
  reason?: string
  danger?: boolean
}) {
  const id = useId()
  return (
    <Dropdown.Item
      className={`action-menu-item${danger ? ' danger' : ''}`}
      aria-disabled={disabled || undefined}
      aria-describedby={reason ? id : undefined}
      onSelect={(event) => {
        if (disabled) event.preventDefault()
        else onSelect()
      }}
    >
      <ActionIcon name={icon} />
      <span>
        {children}
        {reason && <small id={id}>{reason}</small>}
      </span>
    </Dropdown.Item>
  )
}
export const ActionMenuSeparator = Dropdown.Separator
export function CopyMenuItem({
  value,
  label,
  toast,
}: {
  value: string
  label: string
  toast: (message: string) => void
}) {
  return (
    <ActionMenuItem icon="copy" onSelect={() => void copyAction(value, label, toast)}>
      {label}
    </ActionMenuItem>
  )
}
