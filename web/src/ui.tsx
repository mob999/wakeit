import { t as tr } from './i18n'
import { useTranslation } from 'react-i18next'
import React, { type ReactNode } from 'react'
import * as Dialog from '@radix-ui/react-dialog'
import * as Tooltip from '@radix-ui/react-tooltip'
import { LoaderCircle, X } from 'lucide-react'
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
      {loading && <LoaderCircle size={16} className="spin" aria-hidden="true" />}
      {children}
    </button>
  )
})
type IconButtonProps = Omit<ButtonProps, 'variant' | 'children' | 'aria-label'> & {
  label: string
  tooltip?: string
  children: React.ReactElement
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
  { label, tooltip = label, children, className, loading = false, ...props },
  ref,
) {
  useTranslation()
  return (
    <TooltipTarget label={tooltip}>
      <Button
        {...props}
        ref={ref}
        aria-label={label}
        className={['icon-button', className].filter(Boolean).join(' ')}
        loading={loading}
      >
        {!loading &&
          React.cloneElement(children as React.ReactElement<Record<string, unknown>>, {
            'aria-hidden': true,
          })}
      </Button>
    </TooltipTarget>
  )
})
export function TooltipProvider({ children }: { children: ReactNode }) {
  useTranslation()
  return (
    <Tooltip.Provider delayDuration={400} skipDelayDuration={0}>
      {children}
    </Tooltip.Provider>
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
  titleHint,
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
  titleHint?: string
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
            <div>
              <TooltipTarget label={titleHint ?? title} enabled={!!titleHint}>
                <Dialog.Title tabIndex={titleHint ? 0 : undefined}>{title}</Dialog.Title>
              </TooltipTarget>
              {description && <Dialog.Description>{description}</Dialog.Description>}
            </div>
            <div className="modal-heading-actions">
              {actions}
              <Dialog.Close asChild>
                <IconButton label={tr('关闭')}>
                  <X />
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
