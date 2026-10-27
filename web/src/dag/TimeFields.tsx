import { ErrorToast } from '../Toast'
import { useEffect, useRef, useState, type MutableRefObject } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api, time, type TaskConfig, type DagIssue } from '../api'
import { t } from '../i18n'
import { Field, Button } from '../ui'
import { ChoicePicker } from '../ChoicePicker'
import { validDuration } from '../duration'
import { localDateTime, utcDateTime, utcOffset } from './schedule'
export function TimeFields({
  config,
  timezone,
  disabled,
  active = true,
  hideExecution = false,
  onChange,
  validation,
}: {
  config: TaskConfig
  timezone: string
  disabled: boolean
  active?: boolean
  hideExecution?: boolean
  onChange: (config: TaskConfig) => void
  validation?: MutableRefObject<() => DagIssue[]>
}) {
  const [kind, setKind] = useState(
    config.schedule?.every
      ? 'every'
      : config.schedule?.at
        ? 'at'
        : config.schedule
          ? 'cron'
          : 'manual',
  )
  const [timeoutDraft, setTimeoutDraft] = useState(config.execution.timeout ?? '1h')
  const [zone, setZone] = useState(config.schedule?.timezone || timezone)
  const [drafts, setDrafts] = useState(() => ({
    cron: config.schedule?.cron || '0 9 * * *',
    every: config.schedule?.every || '1h',
    at: localDateTime(
      config.schedule?.at || new Date(Date.now() + 3600000).toISOString(),
      config.schedule?.timezone || timezone,
    ),
  }))
  const [editing, setEditing] = useState(false),
    [settled, setSettled] = useState('')
  const zones = useQuery<string[]>({
    queryKey: ['timezones'],
    queryFn: () => api('/timezones'),
    staleTime: Infinity,
    enabled: active,
  })
  const build = (type = kind, values = drafts, tz = zone) =>
    type === 'manual'
      ? null
      : {
          timezone: tz,
          ...(type === 'at'
            ? { at: utcDateTime(values.at, tz) }
            : type === 'every'
              ? { every: values.every }
              : { cron: values.cron }),
        }
  let candidate: TaskConfig['schedule'] = null,
    localError = '',
    zoneError = false
  try {
    new Intl.DateTimeFormat('en', { timeZone: zone }).format()
  } catch {
    zoneError = true
  }
  try {
    if (kind !== 'manual' && zoneError) throw new Error(t('dag.invalidTimezone'))
    candidate = build()
    if (candidate) {
      if (kind === 'cron' && drafts.cron.trim().split(/\s+/).length !== 5)
        localError = t('dag.invalidCron')
      if (kind === 'every' && !validDuration(drafts.every)) localError = t('dag.invalidInterval')
    }
  } catch (e) {
    localError = (e as Error).message
  }
  const signature = JSON.stringify(candidate)
  useEffect(() => {
    const timer = setTimeout(() => setSettled(signature), 500)
    return () => clearTimeout(timer)
  }, [signature, localError])
  const preview = useQuery<string[]>({
    queryKey: ['dag-preview', candidate],
    queryFn: () => api('/schedule/preview', 'POST', candidate),
    enabled: active && !!candidate && !localError && settled === signature,
    retry: false,
    refetchOnWindowFocus: false,
  })
  function apply(type: string, values = drafts, tz = zone) {
    setKind(type)
    setDrafts(values)
    setZone(tz)
    let schedule: TaskConfig['schedule']
    try {
      schedule = build(type, values, tz)
    } catch {
      schedule = { timezone: tz, at: '' }
    }
    onChange({ ...config, schedule })
  }
  const field = zoneError
    ? 'schedule.timezone'
    : kind === 'every'
      ? 'schedule.every'
      : kind === 'at'
        ? 'schedule.at'
        : 'schedule.cron'
  const check = (): DagIssue[] => {
    const errors: DagIssue[] = []
    if (kind !== 'manual' && (localError || preview.error))
      errors.push({ path: field, message: localError || preview.error!.message })
    for (const key of ['timeout', 'start_grace'] as const)
      if (
        (key !== 'timeout' || config.execution.timeout !== null) &&
        !validDuration(config.execution[key] ?? '')
      )
        errors.push({
          path: 'execution.' + key,
          message: t(
            key === 'timeout'
              ? '请填写有效的执行超时（1 秒至 366 天）'
              : '请填写有效的允许延迟（1 秒至 366 天）',
          ),
        })
    return errors
  }
  if (validation) validation.current = check
  return (
    <fieldset className="editor-fields dag-time-page" disabled={disabled}>
      <section className="content-card">
        <header className="card-header">
          <h3>{t('时间')}</h3>
        </header>
        <div className="dag-settings-rows">
          <Field label={t('类型')} fieldKey="schedule.type">
            <ChoicePicker
              label={t('类型')}
              disabled={disabled}
              value={kind}
              options={[
                { value: 'cron', label: 'Cron' },
                { value: 'every', label: t('固定间隔') },
                { value: 'at', label: t('一次性执行') },
                { value: 'manual', label: t('手动执行') },
              ]}
              onChange={(value) => apply(value)}
            />
          </Field>
          {kind !== 'manual' && (
            <>
              <Field label={t('时区')} fieldKey="schedule.timezone">
                <ChoicePicker
                  label={t('时区')}
                  searchable
                  disabled={disabled}
                  value={zone}
                  selectionLabel={zone}
                  pending={zones.isPending}
                  error={zones.error?.message}
                  options={(zones.data || [zone]).map((z) => ({ value: z, label: z }))}
                  onChange={(value) => apply(kind, drafts, value)}
                />
              </Field>
              <Field
                label={
                  kind === 'cron' ? t('Cron 表达式') : kind === 'every' ? t('间隔') : t('执行时间')
                }
                fieldKey={field}
              >
                <input
                  className="mono"
                  type={kind === 'at' ? 'datetime-local' : 'text'}
                  step={kind === 'at' ? 1 : undefined}
                  value={drafts[kind as keyof typeof drafts]}
                  onFocus={() => setEditing(true)}
                  onBlur={() => setEditing(false)}
                  onChange={(e) => {
                    setEditing(true)
                    apply(kind, { ...drafts, [kind]: e.target.value })
                  }}
                />
              </Field>
              <div className="dag-preview-row">
                <span>{t('接下来的执行时间')}</span>
                <div>
                  {!localError &&
                    preview.data?.map((value) => (
                      <div className="mono" key={value}>
                        {time(value, zone)} {zone} ({utcOffset(value, zone)})
                      </div>
                    ))}
                  {(!preview.data || localError) && (
                    <span className="quiet">{preview.isFetching ? t('正在加载…') : '—'}</span>
                  )}
                  {preview.error && <Button onClick={() => preview.refetch()}>{t('重试')}</Button>}
                </div>
              </div>
              {active && !editing && (localError || preview.error) && (
                <ErrorToast>{localError || preview.error!.message}</ErrorToast>
              )}
            </>
          )}
        </div>
      </section>
      {!hideExecution && (
        <section className="content-card">
          <header className="card-header">
            <h3>{t('dag.executionPolicy')}</h3>
          </header>
          <div className="dag-settings-rows">
            <Field label={t('dag.graphTimeout')} fieldKey="execution.timeout">
              <div className="dag-time-limit">
                <label className="check">
                  <input
                    type="checkbox"
                    checked={config.execution.timeout != null}
                    onChange={(e) =>
                      onChange({
                        ...config,
                        execution: {
                          ...config.execution,
                          timeout: e.target.checked ? timeoutDraft : null,
                        },
                      })
                    }
                  />
                  {t('启用')}
                </label>
                {config.execution.timeout != null && (
                  <input
                    aria-label={t('dag.graphTimeout')}
                    value={config.execution.timeout}
                    onChange={(e) => {
                      setTimeoutDraft(e.target.value)
                      onChange({
                        ...config,
                        execution: { ...config.execution, timeout: e.target.value },
                      })
                    }}
                  />
                )}
              </div>
            </Field>
            <Field label={t('允许延迟')} fieldKey="execution.start_grace">
              <input
                value={config.execution.start_grace}
                onChange={(e) =>
                  onChange({
                    ...config,
                    execution: { ...config.execution, start_grace: e.target.value },
                  })
                }
              />
            </Field>
            <Field label={t('错过执行')} fieldKey="execution.misfire">
              <ChoicePicker
                label={t('错过执行')}
                disabled={disabled}
                value={config.execution.misfire}
                options={[
                  { value: 'skip', label: t('跳过') },
                  { value: 'latest', label: t('dag.latest') },
                ]}
                onChange={(value) =>
                  onChange({
                    ...config,
                    execution: { ...config.execution, misfire: value as 'skip' | 'latest' },
                  })
                }
              />
            </Field>
            <Field label={t('执行重叠')} fieldKey="execution.overlap">
              <ChoicePicker
                label={t('执行重叠')}
                disabled={disabled}
                value={config.execution.overlap}
                options={[
                  { value: 'skip', label: t('跳过') },
                  { value: 'allow', label: t('允许并行') },
                ]}
                onChange={(value) =>
                  onChange({
                    ...config,
                    execution: { ...config.execution, overlap: value as 'skip' | 'allow' },
                  })
                }
              />
            </Field>
          </div>
        </section>
      )}
    </fieldset>
  )
}
