import { ErrorToast } from './Toast'
import { t as tr } from './i18n'
import { useTranslation } from 'react-i18next'
import { useEffect, useId, useRef, useState, type ReactNode, type KeyboardEvent } from 'react'
import * as Popover from '@radix-ui/react-popover'
import { Check, ChevronDown, Search } from 'lucide-react'
export type Choice = {
  value: string
  label: string
  keywords?: string
  icon?: ReactNode
  description?: string
  wrapDescription?: boolean
  status?: string
}
export function ChoicePicker({
  id,
  label,
  value,
  options,
  onChange,
  placeholder = tr('请选择'),
  searchable = false,
  disabled = false,
  filterOptions = true,
  pending = false,
  error,
  footer,
  emptyLabel = tr('无匹配选项'),
  selectionLabel,
  onOpenChange,
  onSearchChange,
  open: controlledOpen,
  pinnedOptions = [],
  content,
  indicator,
  'aria-describedby': describedBy,
}: {
  id?: string
  label: string
  value: string
  options: Choice[]
  onChange: (value: string) => void
  placeholder?: string
  searchable?: boolean
  disabled?: boolean
  filterOptions?: boolean
  pending?: boolean
  error?: string
  footer?: ReactNode
  emptyLabel?: string
  selectionLabel?: string
  open?: boolean
  pinnedOptions?: Choice[]
  content?: ReactNode
  indicator?: ReactNode
  onOpenChange?: (open: boolean) => void
  onSearchChange?: (search: string) => void
  'aria-describedby'?: string
}) {
  useTranslation()
  const generated = useId()
  const controlId = id ?? generated
  const listId = `${controlId}-options`
  const [localOpen, setOpen] = useState(false)
  const open = controlledOpen ?? localOpen
  const [search, setSearch] = useState('')
  const [active, setActive] = useState(value)
  const searchRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLDivElement>(null)
  const rows = useRef(new Map<string, HTMLButtonElement>())
  const selected = [...pinnedOptions, ...options].find((o) => o.value === value)
  const filteredOptions =
    pending || error
      ? []
      : !filterOptions
        ? options
        : options.filter((o) =>
            `${o.label} ${o.keywords ?? ''}`
              .toLocaleLowerCase()
              .includes(search.toLocaleLowerCase().trim()),
          )
  const filtered = [...pinnedOptions, ...filteredOptions]
  const signature = filtered.map((o) => o.value).join('\u0000')
  const current = filtered.findIndex((o) => o.value === active)
  const activeIndex =
    current >= 0 ? current : search && filteredOptions.length ? pinnedOptions.length : 0
  const activeId = filtered[activeIndex] ? `${listId}-${activeIndex}` : undefined
  useEffect(() => {
    if (disabled) changeOpen(false)
  }, [disabled])
  useEffect(() => {
    if (open) rows.current.get(filtered[activeIndex]?.value)?.scrollIntoView({ block: 'nearest' })
  }, [open, active, signature])
  useEffect(() => {
    if (open && !content) (searchable ? searchRef.current : listRef.current)?.focus()
  }, [open, !!content])
  function choose(next: string) {
    if (disabled) return
    onChange(next)
    changeOpen(false)
  }
  function changeOpen(next: boolean) {
    if (disabled && next) return
    setOpen(next)
    onOpenChange?.(next)
    if (next) {
      setSearch('')
      onSearchChange?.('')
      setActive(value)
    }
  }
  function keys(event: KeyboardEvent) {
    if (event.nativeEvent.isComposing) return
    if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault()
      if (!filtered.length) return
      const next =
        event.key === 'Home'
          ? 0
          : event.key === 'End'
            ? filtered.length - 1
            : (activeIndex + (event.key === 'ArrowDown' ? 1 : -1) + filtered.length) %
              filtered.length
      setActive(filtered[next].value)
    } else if (event.key === 'Enter' || (!searchable && event.key === ' ')) {
      event.preventDefault()
      if (search.trim() && (pending || error || (current < 0 && !filteredOptions.length))) return
      if (filtered[activeIndex]) choose(filtered[activeIndex].value)
    }
  }
  return (
    <Popover.Root open={open && !disabled} onOpenChange={changeOpen}>
      <ErrorToast error={error} />
      <Popover.Trigger asChild>
        <button
          type="button"
          id={controlId}
          aria-label={label}
          aria-describedby={describedBy}
          role="combobox"
          aria-expanded={open && !disabled}
          aria-controls={content ? undefined : listId}
          disabled={disabled}
          className="model-selector choice-selector"
          onKeyDown={(e) => {
            if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
              e.preventDefault()
              if (!disabled) {
                changeOpen(true)
              }
            }
          }}
        >
          <span className="choice-label">
            {selected?.icon}
            {selectionLabel ?? selected?.label ?? placeholder}
          </span>
          {indicator}
          <ChevronDown size={14} />
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          className="model-popover choice-popover"
          align="start"
          sideOffset={4}
          collisionPadding={12}
          onOpenAutoFocus={(e) => {
            e.preventDefault()
            ;(searchable ? searchRef.current : listRef.current)?.focus()
          }}
        >
          {content ?? (
            <>
              {searchable && (
                <div className="model-search">
                  <Search size={14} />
                  <input
                    ref={searchRef}
                    role="combobox"
                    aria-label={tr('搜索{{v0}}', { v0: label })}
                    placeholder={tr('搜索{{v0}}…', { v0: label })}
                    aria-controls={listId}
                    aria-expanded={open}
                    aria-autocomplete="list"
                    aria-activedescendant={activeId}
                    value={search}
                    onChange={(e) => {
                      setSearch(e.target.value)
                      onSearchChange?.(e.target.value)
                      setActive('\u0001')
                    }}
                    onKeyDown={keys}
                  />
                </div>
              )}
              <div
                role="listbox"
                id={listId}
                aria-label={tr('可选{{v0}}', { v0: label })}
                className="model-options"
                ref={listRef}
                tabIndex={searchable ? -1 : 0}
                aria-activedescendant={!searchable ? activeId : undefined}
                onKeyDown={!searchable ? keys : undefined}
              >
                {filtered.map((option, index) => (
                  <button
                    key={option.value}
                    className={
                      [
                        option.wrapDescription && 'choice-wrap-description',
                        index < pinnedOptions.length && 'choice-pinned',
                      ]
                        .filter(Boolean)
                        .join(' ') || undefined
                    }
                    type="button"
                    role="option"
                    id={`${listId}-${index}`}
                    aria-selected={value === option.value}
                    tabIndex={-1}
                    disabled={disabled}
                    ref={(el) => {
                      if (el) rows.current.set(option.value, el)
                      else rows.current.delete(option.value)
                    }}
                    data-active={index === activeIndex ? '' : undefined}
                    onMouseMove={() => setActive(option.value)}
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => choose(option.value)}
                  >
                    <span className="choice-option-label">
                      {option.icon}
                      <span>
                        {option.label}
                        {option.description && <small className="mono">{option.description}</small>}
                      </span>
                    </span>
                    <span className="choice-option-meta">
                      {option.status && <span className="quiet">{option.status}</span>}
                      {value === option.value && <Check size={13} />}
                    </span>
                  </button>
                ))}
                {!filteredOptions.length && (error || pending || emptyLabel) && (
                  <p className="choice-empty">
                    {error ? null : pending ? tr('正在加载…') : emptyLabel}
                  </p>
                )}
              </div>
              {footer && <div className="choice-footer">{footer}</div>}
            </>
          )}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  )
}
