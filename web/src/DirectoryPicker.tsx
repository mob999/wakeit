import { useState, useId } from 'react'
import { Folder, ChevronRight } from 'lucide-react'
import { type Runner } from './api'
import { Modal } from './ui'
import { DirectoryBrowser } from './directories/DirectoryBrowser'
const lastDirectory = new Map<string, string>()
export function DirectoryPicker({
  runner,
  value,
  onChange,
}: {
  runner?: Runner
  value: string
  onChange: (v: string) => void
}) {
  const id = useId()
  const [open, setOpen] = useState(false)
  return (
    <div className="field">
      <label htmlFor={id}>工作目录</label>
      <button
        id={id}
        type="button"
        aria-label="选择目录"
        className="directory-selector"
        disabled={!runner?.online || !runner.directory_roots?.length}
        onClick={() => setOpen(true)}
      >
        <Folder size={15} />
        <span className={value ? 'directory-path mono' : 'directory-path quiet'} title={value}>
          {value || (!runner ? '先选择 Runner' : '选择工作目录')}
        </span>
        <span className="quiet">浏览</span>
        <ChevronRight size={14} />
      </button>
      {runner && !runner.online && <small>设备离线，无法浏览。</small>}
      {runner?.online && !runner.directory_roots?.length && <small>此设备尚未开放工作目录。</small>}
      <Modal open={open} onOpenChange={setOpen} title="选择工作目录">
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
              onChange(path)
              setOpen(false)
            }}
          />
        )}
      </Modal>
    </div>
  )
}
