import { t as tr } from './i18n'
import { useTranslation } from 'react-i18next'
import { useState, useId, useEffect } from 'react'
import { Folder, ChevronRight } from './icons'
import { type Runner } from './api'
import { Modal } from './ui'
import { DirectoryBrowser } from './directories/DirectoryBrowser'
const lastDirectory = new Map<string, string>()
export function DirectoryPicker({
  runner,
  value,
  onChange,
  disabled = false,
}: {
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
    <div className="field" data-field="cwd">
      <label htmlFor={id}>{tr('工作目录')}</label>
      <button
        id={id}
        type="button"
        aria-label={tr('选择目录')}
        className="directory-selector"
        disabled={disabled || !runner?.online || !runner.directory_roots?.length}
        onClick={() => setOpen(true)}
      >
        <Folder size={15} />
        <span className={value ? 'directory-path mono' : 'directory-path quiet'} title={value}>
          {value || (!runner ? tr('先选择 Runner') : tr('选择工作目录'))}
        </span>
        <span className="quiet">{tr('浏览')}</span>
        <ChevronRight size={14} />
      </button>
      {runner && !runner.online && <small>{tr('设备离线，无法浏览。')}</small>}
      {runner?.online && !runner.directory_roots?.length && (
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
