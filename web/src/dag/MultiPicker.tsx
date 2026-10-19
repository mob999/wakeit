import { useState } from 'react'
import * as Popover from '@radix-ui/react-popover'
import { Check, ChevronDown, Search } from 'lucide-react'
import { t } from '../i18n'
export function MultiPicker({
  label,
  value,
  options,
  disabled = false,
  onChange,
  empty,
}: {
  label: string
  value: string[]
  options: { id: string; name: string; disabled?: boolean; reason?: string }[]
  disabled?: boolean
  onChange: (ids: string[]) => void
  empty?: string
}) {
  const [search, setSearch] = useState(''),
    [open, setOpen] = useState(false)
  return (
    <Popover.Root
      open={open && !disabled}
      onOpenChange={(v) => {
        setOpen(v)
        setSearch('')
      }}
    >
      <Popover.Trigger asChild>
        <button
          type="button"
          className="model-selector choice-selector"
          role="combobox"
          aria-expanded={open && !disabled}
          aria-label={label}
          disabled={disabled}
        >
          <span className="choice-label">
            {value.length
              ? value.map((id) => options.find((n) => n.id === id)?.name || id).join(', ')
              : (empty ?? t('dag.selectNodes'))}
          </span>
          <ChevronDown size={14} />
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          className="model-popover choice-popover"
          align="start"
          sideOffset={4}
          collisionPadding={12}
        >
          <div className="model-search">
            <Search size={14} />
            <input
              aria-label={t('dag.search')}
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              autoFocus
            />
          </div>
          <div
            className="model-options"
            role="listbox"
            aria-label={label}
            aria-multiselectable="true"
          >
            {options
              .filter((n) => n.name.toLocaleLowerCase().includes(search.toLocaleLowerCase()))
              .map((n) => (
                <button
                  type="button"
                  role="option"
                  key={n.id}
                  aria-selected={value.includes(n.id)}
                  disabled={disabled || n.disabled}
                  title={n.reason}
                  onKeyDown={(e) => {
                    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
                      e.preventDefault()
                      const buttons = [
                        ...e.currentTarget.parentElement!.querySelectorAll<HTMLButtonElement>(
                          'button:not(:disabled)',
                        ),
                      ]
                      buttons[
                        (buttons.indexOf(e.currentTarget) +
                          (e.key === 'ArrowDown' ? 1 : -1) +
                          buttons.length) %
                          buttons.length
                      ]?.focus()
                    }
                  }}
                  onClick={() =>
                    !disabled &&
                    onChange(
                      value.includes(n.id) ? value.filter((id) => id !== n.id) : [...value, n.id],
                    )
                  }
                >
                  <span className="choice-option-label">
                    {n.name}
                    {n.reason && <small>{n.reason}</small>}
                  </span>
                  <span className="choice-option-meta">
                    {value.includes(n.id) && <Check size={13} />}
                  </span>
                </button>
              ))}
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  )
}
