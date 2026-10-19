import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Clock, ChevronRight } from 'lucide-react'
import { api, time, type TaskConfig } from '../api'
import { t } from '../i18n'
import { Field } from '../ui'
import { ChoicePicker } from '../ChoicePicker'
export function TimeFields({
  config,
  timezone,
  disabled,
  onChange,
}: {
  config: TaskConfig
  timezone: string
  disabled: boolean
  onChange: (config: TaskConfig) => void
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
  const preview = useQuery<string[]>({
    queryKey: ['preview', config.schedule],
    queryFn: () => api('/schedule/preview', 'POST', config.schedule),
    enabled: !!config.schedule,
    retry: false,
  })
  const patch = (value: Partial<TaskConfig>) => onChange({ ...config, ...value })
  return (
    <fieldset className="editor-fields dag-time-page" disabled={disabled}>
      <div className="form-grid">
        <Field label={t('类型')}>
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
            onChange={(next) => {
              setKind(next)
              patch({
                schedule:
                  next === 'manual'
                    ? null
                    : {
                        timezone: config.schedule?.timezone ?? timezone,
                        ...(next === 'cron'
                          ? { cron: '0 9 * * *' }
                          : next === 'every'
                            ? { every: '1h' }
                            : { at: new Date(Date.now() + 3600000).toISOString() }),
                      },
              })
            }}
          />
        </Field>
        {config.schedule && (
          <Field label={t('时区')}>
            <input
              value={config.schedule.timezone}
              onChange={(e) =>
                patch({ schedule: { ...config.schedule, timezone: e.target.value } })
              }
            />
          </Field>
        )}
      </div>
      {config.schedule && (
        <>
          <Field
            label={
              kind === 'cron' ? t('Cron 表达式') : kind === 'every' ? t('间隔') : t('执行时间')
            }
          >
            <input
              className="mono"
              value={config.schedule.cron ?? config.schedule.every ?? config.schedule.at ?? ''}
              onChange={(e) => patch({ schedule: { ...config.schedule!, [kind]: e.target.value } })}
            />
          </Field>
          <div className="preview">
            <Clock size={13} />
            <div>
              <span>{t('接下来的执行时间')}</span>
              {preview.error ? (
                <small className="error">{preview.error.message}</small>
              ) : (
                preview.data?.map((v) => (
                  <small key={v}>
                    {time(v, config.schedule!.timezone)} {config.schedule!.timezone}
                  </small>
                ))
              )}
            </div>
          </div>
        </>
      )}
      <details className="advanced-settings">
        <summary>
          <ChevronRight size={14} />
          {t('高级设置')}
        </summary>
        <div className="advanced-body">
          <div className="form-grid">
            {(['timeout', 'start_grace'] as const).map((key) => (
              <Field key={key} label={key === 'timeout' ? t('dag.graphTimeout') : t('允许延迟')}>
                <input
                  value={config.execution[key]}
                  onChange={(e) =>
                    patch({ execution: { ...config.execution, [key]: e.target.value } })
                  }
                />
              </Field>
            ))}
            <Field label={t('错过执行')}>
              <ChoicePicker
                label={t('错过执行')}
                disabled={disabled}
                value={config.execution.misfire}
                options={[
                  { value: 'skip', label: t('跳过') },
                  { value: 'latest', label: t('dag.latest') },
                ]}
                onChange={(value) =>
                  patch({ execution: { ...config.execution, misfire: value as 'skip' | 'latest' } })
                }
              />
            </Field>
            <Field label={t('执行重叠')}>
              <ChoicePicker
                label={t('执行重叠')}
                disabled={disabled}
                value={config.execution.overlap}
                options={[
                  { value: 'skip', label: t('跳过') },
                  { value: 'allow', label: t('允许并行') },
                ]}
                onChange={(value) =>
                  patch({ execution: { ...config.execution, overlap: value as 'skip' | 'allow' } })
                }
              />
            </Field>
          </div>
        </div>
      </details>
    </fieldset>
  )
}
