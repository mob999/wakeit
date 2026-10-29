import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useState, useId, useEffect } from 'react'
import { Folder, ChevronRight } from '../components/icons'
import { type Runner } from '../api/types'
import { Modal } from '../components/ui'
import { DirectoryBrowser } from './DirectoryBrowser'
const lastDirectory = new Map<string, string>()
export function DirectoryPicker({
  runner,
  value,
  onChange,
  disabled = false,
  compact = false,
}: {
  compact?: boolean
  runner?: Runner
  disabled?: boolean
  value: string
  onChange: (v: string) => void
}) {
  useTranslation()
  const id = useId()
  const [open, setOpen] = useState(false)
  useEffect(() => {
    if (disabled) setOpen(false)
  }, [disabled])
  return (
    <div className={compact ? 'directory-browse-control' : 'field'} data-field="cwd">
      {!compact && <label htmlFor={id}>{tr('工作目录')}</label>}
      <button
        id={id}
        type="button"
        aria-label={tr('选择目录')}
        className={compact ? 'directory-selector is-compact' : 'directory-selector'}
        disabled={disabled || !runner?.online || !runner.directory_roots?.length}
        onClick={() => setOpen(true)}
      >
        <Folder size={15} />
        {!compact && (
          <>
            <span
              className={value ? 'directory-path mono' : 'directory-path quiet text-muted'}
              title={value}
            >
              {value || (!runner ? tr('先选择 Runner') : tr('选择工作目录'))}
            </span>
            <span className="quiet text-muted">{tr('浏览')}</span>
            <ChevronRight size={14} />
          </>
        )}
      </button>
      {!compact && runner && !runner.online && <small>{tr('设备离线，无法浏览。')}</small>}
      {!compact && runner?.online && !runner.directory_roots?.length && (
        <small>{tr('此设备尚未开放工作目录。')}</small>
      )}
      <Modal open={open} onOpenChange={setOpen} title={tr('选择工作目录')}>
        {open && runner && (
          <DirectoryBrowser
            key={runner.id}
            runner={runner}
            initial={
              value ||
              lastDirectory.get(runner.id) ||
              (runner.directory_roots.length === 1 ? runner.directory_roots[0] : null)
            }
            onCancel={() => setOpen(false)}
            onSelect={(path) => {
              lastDirectory.set(runner.id, path)
              if (!disabled) onChange(path)
              setOpen(false)
            }}
          />
        )}
      </Modal>
    </div>
  )
}
