import { ErrorToast } from '../components/Toast'
import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useId, useState, type ReactNode } from 'react'
import { useQuery } from '@tanstack/react-query'
import * as Popover from '@radix-ui/react-popover'
import { Check, ChevronDown, Search } from '../components/icons'
import { api } from '../api/client'
import { Button } from '../components/ui'
import { RequestState } from './shared'
export type WorkspaceDraft = {
  timezone: string
  log_retention_days: string
  run_retention_days: string
}
export function draftErrors(draft: WorkspaceDraft) {
  const errors: Partial<Record<keyof WorkspaceDraft, string>> = {}
  if (!draft.timezone) errors.timezone = tr('请选择时区')
  for (const key of ['log_retention_days', 'run_retention_days'] as const) {
    if (!/^\d+$/.test(draft[key]) || Number(draft[key]) < 1 || Number(draft[key]) > 3650)
      errors[key] = tr('请输入 1–3650 的整数')
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
  useTranslation()
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
            aria-label={tr('默认时区')}
            aria-expanded={open}
            aria-controls={`${id}-options`}
          >
            <span className="mono">{value || tr('选择时区')}</span>
            <ChevronDown size={14} />
          </button>
        </Popover.Trigger>
        <Popover.Portal>
          <Popover.Content className="model-popover timezone-popover" align="end" sideOffset={4}>
            <div className="model-search">
              <Search size={14} />
              <input
                aria-label={tr('搜索时区')}
                placeholder={tr('搜索时区…')}
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
              <div
                className="model-options"
                role="listbox"
                id={`${id}-options`}
                aria-label={tr('时区')}
              >
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
                  <p className="quiet text-muted padded p-5">{tr('无匹配时区')}</p>
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
  readOnly = false,
  draft,
  dirty,
  pending,
  error,
  onChange,
  onCancel,
  onSave,
  workspaceName,
  workspaceActions,
}: {
  readOnly?: boolean
  draft: WorkspaceDraft
  dirty: boolean
  pending: boolean
  error: string
  onChange: (draft: WorkspaceDraft) => void
  onCancel: () => void
  onSave: () => void
  workspaceName?: ReactNode
  workspaceActions?: ReactNode
}) {
  useTranslation()
  const errors = draftErrors(draft)
  return (
    <form
      className="workspace-settings content-card"
      onSubmit={(e) => {
        e.preventDefault()
        if (!readOnly) onSave()
      }}
    >
      <div className="form-card-head card-header">
        <h3>{tr('设置')}</h3>
        {workspaceActions}
      </div>
      {workspaceName}
      <div className="setting-row">
        <label>{tr('默认时区')}</label>
        {readOnly ? (
          <span className="mono">{draft.timezone}</span>
        ) : (
          <TimezonePicker
            value={draft.timezone}
            disabled={pending}
            onChange={(timezone) => onChange({ ...draft, timezone })}
          />
        )}
      </div>
      {(['log_retention_days', 'run_retention_days'] as const).map((key, index) => (
        <div className="setting-row" key={key}>
          <label htmlFor={readOnly ? undefined : `setting-${key}`}>
            {index === 0 ? tr('日志保留') : tr('运行记录保留')}
          </label>
          {readOnly ? (
            <span>
              {draft[key]} {tr('unit.days', { count: Number(draft[key]) })}
            </span>
          ) : (
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
                <span>{tr('unit.days', { count: Number(draft[key]) })}</span>
              </div>
              {errors[key] && <ErrorToast id={`${key}-error`} error={errors[key]} />}
            </div>
          )}
        </div>
      ))}
      {!readOnly && dirty && (
        <div className="settings-savebar">
          {error && <ErrorToast error={error} />}
          <div>
            <Button type="button" disabled={pending} onClick={onCancel}>
              {tr('取消')}
            </Button>
            <Button
              type="submit"
              variant="solid"
              disabled={pending || Object.keys(errors).length > 0}
            >
              {pending ? tr('正在保存…') : tr('保存')}
            </Button>
          </div>
        </div>
      )}
    </form>
  )
}
