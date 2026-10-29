import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import React, { type ReactNode } from 'react'
import * as Dialog from '@radix-ui/react-dialog'
import * as Tooltip from '@radix-ui/react-tooltip'
import { ActionIcon } from '../actions/ActionIcon'
import '../actions/actions.css'
import { actionStatusEvent } from '../actions/announcements'
type ButtonProps = React.ComponentPropsWithoutRef<'button'> & {
  variant?: 'ghost' | 'solid'
  danger?: boolean
  loading?: boolean
}
export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  {
    children,
    variant = 'ghost',
    danger = false,
    loading = false,
    disabled,
    className,
    type = 'button',
    ...props
  },
  ref,
) {
  useTranslation()
  return (
    <button
      {...props}
      ref={ref}
      type={type}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      className={['button', variant, danger && 'danger', className].filter(Boolean).join(' ')}
    >
      {loading && <ActionIcon name="loading" className="spin" />}
      {children}
    </button>
  )
})
export type IconButtonProps = Omit<ButtonProps, 'variant' | 'children' | 'aria-label'> & {
  label: string
  tooltip?: string
  children: React.ReactElement
  text?: string
  description?: string
  disabledReason?: string
  tooltipEnabled?: boolean
}
export function TooltipTarget({
  label,
  children,
  enabled = true,
}: {
  label: string
  children: React.ReactElement
  enabled?: boolean
}) {
  useTranslation()
  if (!enabled) return children
  return (
    <Tooltip.Root>
      <Tooltip.Trigger asChild>{children}</Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content className="tooltip" sideOffset={6} collisionPadding={8}>
          {label}
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  )
}
export const IconButton = React.forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  {
    label,
    tooltip = label,
    children,
    text,
    description,
    disabledReason,
    tooltipEnabled = true,
    className,
    loading = false,
    disabled,
    onClick,
    onKeyDown,
    ...props
  },
  ref,
) {
  useTranslation()
  const id = React.useId()
  const blocked = !!disabled && !!disabledReason
  const details = [description, blocked && disabledReason].filter(Boolean).join(' ')
  return (
    <>
      <TooltipTarget
        label={blocked ? `${label} — ${disabledReason}` : tooltip}
        enabled={tooltipEnabled}
      >
        <Button
          {...props}
          ref={ref}
          aria-label={label}
          aria-describedby={
            [props['aria-describedby'], details && id].filter(Boolean).join(' ') || undefined
          }
          aria-disabled={blocked || undefined}
          disabled={disabled && !blocked}
          className={['icon-button', text && 'has-action-text', className]
            .filter(Boolean)
            .join(' ')}
          loading={loading}
          onClick={(event) => {
            if (blocked) {
              event.preventDefault()
              event.stopPropagation()
              return
            }
            onClick?.(event)
          }}
          onKeyDown={(event) => {
            if (blocked && ['Enter', ' '].includes(event.key)) {
              event.preventDefault()
              event.stopPropagation()
              return
            }
            onKeyDown?.(event)
          }}
        >
          {!loading &&
            React.cloneElement(children as React.ReactElement<Record<string, unknown>>, {
              'aria-hidden': true,
            })}
          {text && <span className="action-label">{text}</span>}
          {!text && (
            <span className="action-narrow-label" aria-hidden="true">
              {label}
            </span>
          )}
        </Button>
      </TooltipTarget>
      {details && (
        <span className="sr-only" id={id}>
          {details}
        </span>
      )}
    </>
  )
})
export function TooltipProvider({ children }: { children: ReactNode }) {
  useTranslation()
  return (
    <Tooltip.Provider delayDuration={400} skipDelayDuration={0}>
      {children}
      <ActionAnnouncements />
    </Tooltip.Provider>
  )
}
function ActionAnnouncements() {
  const [message, setMessage] = React.useState('')
  React.useEffect(() => {
    let frame = 0
    const receive = (event: Event) => {
      setMessage('')
      cancelAnimationFrame(frame)
      frame = requestAnimationFrame(() => setMessage((event as CustomEvent<string>).detail))
    }
    window.addEventListener(actionStatusEvent, receive)
    return () => {
      window.removeEventListener(actionStatusEvent, receive)
      cancelAnimationFrame(frame)
    }
  }, [])
  return (
    <span className="sr-only" role="status" aria-live="polite" aria-atomic="true">
      {message}
    </span>
  )
}
export function Modal({
  open,
  onOpenChange,
  title,
  description,
  actions,
  children,
  wide = false,
  onOpenAutoFocus,
  onCloseAutoFocus,
  className,
  terminal = false,
  hideTitle = false,
  closePending = false,
}: {
  open: boolean
  onOpenChange: (v: boolean) => void
  title: string
  description?: string
  actions?: ReactNode
  children: ReactNode
  wide?: boolean
  className?: string
  terminal?: boolean
  hideTitle?: boolean
  closePending?: boolean
  onOpenAutoFocus?: (event: Event) => void
  onCloseAutoFocus?: (event: Event) => void
}) {
  useTranslation()
  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="overlay" />
        <Dialog.Content
          className={`modal ${wide ? 'wide' : ''} ${className ?? ''}`}
          onEscapeKeyDown={terminal ? (event) => event.preventDefault() : undefined}
          aria-keyshortcuts={terminal ? 'Control+Shift+F6' : undefined}
          aria-description={terminal ? tr('terminal.focusControls') : undefined}
          onKeyDownCapture={
            terminal
              ? (event) => {
                  if (event.key === 'F6' && event.ctrlKey && event.shiftKey) {
                    event.preventDefault()
                    const root = event.currentTarget
                    const inHeader = (event.target as Element).closest('.modal-heading')
                    root
                      .querySelector<HTMLElement>(
                        inHeader ? '.xterm-helper-textarea' : '.modal-heading button',
                      )
                      ?.focus()
                  }
                }
              : undefined
          }
          onOpenAutoFocus={onOpenAutoFocus}
          onCloseAutoFocus={onCloseAutoFocus}
          onInteractOutside={(event) => {
            const target = event.detail.originalEvent.target
            if (terminal || (target instanceof Element && target.closest('.toast')))
              event.preventDefault()
          }}
          {...(!description ? { 'aria-describedby': undefined } : {})}
        >
          <div className="modal-heading">
            <div className={hideTitle ? 'sr-only' : undefined}>
              <Dialog.Title>{title}</Dialog.Title>
              {description && <Dialog.Description>{description}</Dialog.Description>}
            </div>
            <div className="modal-heading-actions">
              {actions}
              <Dialog.Close asChild>
                <IconButton
                  label={closePending ? tr('terminal.closing') : tr('关闭')}
                  loading={closePending}
                >
                  <ActionIcon name="close" />
                </IconButton>
              </Dialog.Close>
            </div>
          </div>
          {children}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
export function Field({
  label,
  children,
  hint,
  fieldKey,
}: {
  fieldKey?: string
  label: string
  children: ReactNode
  hint?: string
}) {
  useTranslation()
  const id = React.useId()
  const child = React.isValidElement(children)
    ? React.cloneElement(children as React.ReactElement<Record<string, unknown>>, {
        id,
        'aria-describedby': hint ? `${id}-hint` : undefined,
      })
    : children
  return (
    <div className="field" data-field={fieldKey}>
      <label htmlFor={id}>{label}</label>
      {child}
      {hint && <small id={`${id}-hint`}>{hint}</small>}
    </div>
  )
}

export function focusEditorField(root: HTMLElement | null, label: string) {
  const field = [...(root?.querySelectorAll<HTMLLabelElement>('label') ?? [])]
    .find((item) => item.textContent?.trim() === label)
    ?.closest('.field')
  const control =
    field?.querySelector<HTMLElement>('input, textarea, button, select') ??
    root?.querySelector<HTMLElement>('[aria-invalid=true]')
  const advanced = control?.closest('details')
  if (advanced) advanced.open = true
  control?.focus()
}
