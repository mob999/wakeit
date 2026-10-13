import { useId, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import * as Popover from '@radix-ui/react-popover'
import { Check, ChevronDown, Search } from 'lucide-react'
import { api } from '../api'
import { Button } from '../ui'
import { RequestState } from './shared'
export type WorkspaceDraft = {
  timezone: string
  log_retention_days: string
  run_retention_days: string
}
export function draftErrors(draft: WorkspaceDraft) {
  const errors: Partial<Record<keyof WorkspaceDraft, string>> = {}
  if (!draft.timezone) errors.timezone = '请选择时区'
  for (const key of ['log_retention_days', 'run_retention_days'] as const) {
    if (!/^\d+$/.test(draft[key]) || Number(draft[key]) < 1 || Number(draft[key]) > 3650)
      errors[key] = '请输入 1–3650 的整数'
  }
  return errors
}
function TimezonePicker({
  value,
  onChange,
  disabled,
}: {
  value: string
  onChange: (value: string) => void
  disabled: boolean
}) {
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState('')
  const id = useId()
  const zones = useQuery<string[]>({
    queryKey: ['timezones'],
    queryFn: () => api('/timezones'),
    staleTime: Infinity,
  })
  return (
    <div className="timezone-control">
      <Popover.Root
        open={open}
        onOpenChange={(v) => {
          setOpen(v)
          setSearch('')
        }}
      >
        <Popover.Trigger asChild>
          <button
            type="button"
            id={id}
            disabled={disabled}
            className="timezone-selector"
            role="combobox"
            aria-label="默认时区"
            aria-expanded={open}
            aria-controls={`${id}-options`}
          >
            <span className="mono">{value || '选择时区'}</span>
            <ChevronDown size={14} />
          </button>
        </Popover.Trigger>
        <Popover.Portal>
          <Popover.Content className="model-popover timezone-popover" align="end" sideOffset={4}>
            <div className="model-search">
              <Search size={14} />
              <input
                aria-label="搜索时区"
                placeholder="搜索时区…"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
            </div>
            {zones.error || zones.isPending ? (
              <RequestState
                pending={zones.isPending}
                error={zones.error}
                retry={() => zones.refetch()}
              />
            ) : (
              <div className="model-options" role="listbox" id={`${id}-options`} aria-label="时区">
                {zones.data
                  ?.filter((z) => z.toLowerCase().includes(search.toLowerCase()))
                  .map((z) => (
                    <button
                      type="button"
                      key={z}
                      role="option"
                      aria-selected={z === value}
                      className="model-option"
                      onClick={() => {
                        onChange(z)
                        setOpen(false)
                      }}
                    >
                      <span className="mono">{z}</span>
                      {z === value && <Check size={13} />}
                    </button>
                  ))}
                {!zones.data?.some((z) => z.toLowerCase().includes(search.toLowerCase())) && (
                  <p className="quiet padded">无匹配时区</p>
                )}
              </div>
            )}
          </Popover.Content>
        </Popover.Portal>
      </Popover.Root>
    </div>
  )
}
export function WorkspaceSettingsView({
  draft,
  dirty,
  pending,
  error,
  onChange,
  onCancel,
  onSave,
}: {
  draft: WorkspaceDraft
  dirty: boolean
  pending: boolean
  error: string
  onChange: (draft: WorkspaceDraft) => void
  onCancel: () => void
  onSave: () => void
}) {
  const errors = draftErrors(draft)
  return (
    <form
      className="workspace-settings content-card"
      onSubmit={(e) => {
        e.preventDefault()
        onSave()
      }}
    >
      <div className="form-card-head table-head">
        <span>设置</span>
        <span>值</span>
      </div>
      <div className="setting-row">
        <label>默认时区</label>
        <TimezonePicker
          value={draft.timezone}
          disabled={pending}
          onChange={(timezone) => onChange({ ...draft, timezone })}
        />
      </div>
      {(['log_retention_days', 'run_retention_days'] as const).map((key, index) => (
        <div className="setting-row" key={key}>
          <label htmlFor={`setting-${key}`}>{index === 0 ? '日志保留' : '运行记录保留'}</label>
          <div className="setting-control">
            <div className="retention-input">
              <input
                id={`setting-${key}`}
                aria-invalid={!!errors[key]}
                aria-describedby={errors[key] ? `${key}-error` : undefined}
                type="number"
                disabled={pending}
                min={1}
                max={3650}
                step={1}
                value={draft[key]}
                onChange={(e) => onChange({ ...draft, [key]: e.target.value })}
              />
              <span>天</span>
            </div>
            {errors[key] && (
              <small id={`${key}-error`} className="error">
                {errors[key]}
              </small>
            )}
          </div>
        </div>
      ))}
      {dirty && (
        <div className="settings-savebar">
          {error && (
            <span className="error" role="alert">
              {error}
            </span>
          )}
          <div>
            <Button type="button" disabled={pending} onClick={onCancel}>
              取消
            </Button>
            <Button
              type="submit"
              variant="solid"
              disabled={pending || Object.keys(errors).length > 0}
            >
              {pending ? '正在保存…' : '保存'}
            </Button>
          </div>
        </div>
      )}
    </form>
  )
}
