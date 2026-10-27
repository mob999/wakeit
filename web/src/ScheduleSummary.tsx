import type { TaskConfig } from './api'
import { time } from './api'
import { t } from './i18n'

export function ScheduleSummary({
  schedule,
  enabled,
}: Pick<TaskConfig, 'schedule'> & { enabled?: boolean }) {
  const rule = schedule?.cron
    ? `Cron ${schedule.cron}`
    : schedule?.every
      ? t('taskOverview.every', { duration: schedule.every })
      : schedule?.at
        ? time(schedule.at, schedule.timezone)
        : t('手动执行')
  return (
    <span className="creation-schedule-summary">
      <span>{rule}</span>
      {schedule && <span className="quiet">{schedule.timezone}</span>}
      {enabled === false && <span className="quiet">{t('actions.taskDisabled')}</span>}
    </span>
  )
}
