import { useContext, useEffect, useId, useState, type ReactNode } from 'react'
import * as Popover from '@radix-ui/react-popover'
import { IconButton, TooltipTarget } from '../ui'
import { PreviewActivity } from '../files/PreviewActivity'
import { ActionIcon } from './ActionIcon'

export function InfoTrigger({
  label,
  tooltip = label,
  children,
  content,
  className,
}: {
  label: string
  tooltip?: string
  children?: ReactNode
  content: ReactNode
  className?: string
}) {
  const [open, setOpen] = useState(false)
  const pin = useContext(PreviewActivity),
    id = useId()
  useEffect(() => {
    pin(id, open)
    return () => pin(id, false)
  }, [id, pin, open])
  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      {children ? (
        <TooltipTarget label={tooltip} enabled={!open}>
          <Popover.Trigger asChild>
            <button type="button" className={className} aria-label={label}>
              {children}
            </button>
          </Popover.Trigger>
        </TooltipTarget>
      ) : (
        <Popover.Trigger asChild>
          <IconButton label={label} tooltip={tooltip} tooltipEnabled={!open}>
            <ActionIcon name="info" />
          </IconButton>
        </Popover.Trigger>
      )}
      <Popover.Portal>
        <Popover.Content
          className="action-info-popover"
          align="end"
          sideOffset={6}
          collisionPadding={12}
          aria-label={label}
        >
          <h4>{label}</h4>
          {content}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  )
}
