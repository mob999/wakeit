import { useTranslation } from 'react-i18next'
import type { MomentEntry } from '../api/types'
import { TooltipTarget } from '../components/ui'
import { locale, stateNames } from '../i18n'
import { answerSummary } from './summary'

export function MomentRow({
  entry,
  timezone,
  onOpen,
}: {
  entry: MomentEntry
  timezone: string
  onOpen: (entry: MomentEntry) => void
}) {
  const { t } = useTranslation()
  const preview = answerSummary(entry)
  const summary = preview.title || entry.name
  const status = stateNames[entry.state] ?? entry.state
  return (
    <article className="moment-entry" data-moment-id={entry.id}>
      <time className="moment-time" dateTime={entry.finished_at}>
        {new Date(entry.finished_at).toLocaleTimeString(locale(), {
          timeZone: timezone,
          hourCycle: 'h23',
          hour: '2-digit',
          minute: '2-digit',
        })}
      </time>
      <TooltipTarget label={entry.reason ? `${status}：${entry.reason}` : status}>
        <span className={`moment-node status ${entry.state}`} role="img" aria-label={status}>
          <i />
        </span>
      </TooltipTarget>
      <button
        className="moment-summary"
        type="button"
        onClick={() => onOpen(entry)}
        aria-label={`${summary}，${status}${entry.creator ? `，@${entry.creator.username}` : ''}`}
      >
        <TooltipTarget label={[summary, preview.continuation].filter(Boolean).join(' — ')}>
          <span className="moment-summary-text">
            <span className="moment-summary-title">{summary}</span>
            {preview.continuation && (
              <span className="moment-summary-continuation">{preview.continuation}</span>
            )}
          </span>
        </TooltipTarget>
        {entry.creator && (
          <TooltipTarget label={t('moments.creator', { name: entry.creator.username })}>
            <span className="moment-author">@{entry.creator.username}</span>
          </TooltipTarget>
        )}
      </button>
    </article>
  )
}
