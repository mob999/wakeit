import { useQuery } from '@tanstack/react-query'
import { Link } from '@tanstack/react-router'
import { useTranslation } from 'react-i18next'
import { api } from '../api/client'
import type { MomentOverview, MomentAttention } from '../api/types'
import { Button, TooltipTarget } from '../components/ui'
import { ErrorToast } from '../components/Toast'
import { locale, stateNames } from '../i18n'
import { manual } from './MomentsPage'
import { MomentTrend } from './MomentTrend'

export function MomentsOverview() {
  const { t } = useTranslation()
  const query = useQuery({
    queryKey: ['moments', 'overview'],
    queryFn: () => api<MomentOverview>('/moments/overview?days=7'),
    ...manual,
  })
  const data = query.data
  const totals = data?.daily.reduce(
    (sum, day) => ({
      succeeded: sum.succeeded + day.succeeded,
      total: sum.total + day.succeeded + day.failed + day.warning,
    }),
    { succeeded: 0, total: 0 },
  )
  function attention(item: MomentAttention) {
    if (item.kind === 'runner')
      return t('moments.offlineImpact', { tasks: item.task_count, runs: item.run_count })
    return [stateNames[item.state] ?? item.state, item.reason].filter(Boolean).join('：')
  }
  function scheduled(at: string) {
    const zone = data!.timezone
    const day = (value: string) => new Date(value).toLocaleDateString('sv-SE', { timeZone: zone })
    const time = new Date(at).toLocaleTimeString(locale(), {
      timeZone: zone,
      hourCycle: 'h23',
      hour: '2-digit',
      minute: '2-digit',
    })
    return day(at) === day(data!.as_of)
      ? `${t('今天')} ${time}`
      : new Date(at).toLocaleString(locale(), {
          timeZone: zone,
          hourCycle: 'h23',
          month: '2-digit',
          day: '2-digit',
          hour: '2-digit',
          minute: '2-digit',
        })
  }
  return (
    <aside className="content-card moments-overview" aria-label={t('moments.overview')}>
      <header className="card-header">
        <h3>{t('moments.overview')}</h3>
        <span className="quiet text-muted">{t('moments.lastDays', { count: 7 })}</span>
      </header>
      {query.isPending && <p className="moments-state">{t('正在加载…')}</p>}
      {query.error && (
        <ErrorToast
          error={query.error}
          actions={<Button onClick={() => query.refetch()}>{t('重试')}</Button>}
        />
      )}
      {data && (
        <>
          <div className="moment-metrics">
            <TooltipTarget label={t('moments.successDefinition')}>
              <div className="moment-success-rate" tabIndex={0}>
                <span className="text-muted">{t('moments.successRate')}</span>
                <strong>
                  {totals!.total ? (
                    <>
                      {Math.round((100 * totals!.succeeded) / totals!.total)}
                      <small>%</small>
                    </>
                  ) : (
                    '—'
                  )}
                </strong>
                <span className="text-muted">
                  {t('moments.successSample', {
                    succeeded: totals!.succeeded,
                    total: totals!.total,
                  })}
                </span>
              </div>
            </TooltipTarget>
            <div className="moment-current">
              <div>
                <span>{t('运行中')}</span>
                <strong>{data.current.running}</strong>
              </div>
              <div>
                <span>{t('moments.queued')}</span>
                <strong>{data.current.queued}</strong>
              </div>
            </div>
          </div>
          <section className="moment-trend-section">
            <div className="moment-section-title">
              <h4>{t('moments.trend')}</h4>
              <div className="moment-trend-legend">
                <span>
                  <i className="success" />
                  {t('成功')}
                </span>
                <span>
                  <i className="failure" />
                  {t('失败')}
                </span>
              </div>
            </div>
            <MomentTrend days={data.daily} />
          </section>
          <section className="moment-dashboard-section">
            <div className="moment-section-title">
              <h4>{t('moments.attention')}</h4>
              <span className="text-muted">{data.attention_total}</span>
            </div>
            {data.attention.length ? (
              data.attention.map((item) => {
                const warning =
                  item.kind === 'runner' || item.kind === 'unknown' || item.state === 'warning'
                const detail =
                  item.kind === 'runner'
                    ? attention(item)
                    : item.reason || stateNames[item.state] || item.state
                return (
                  <TooltipTarget
                    key={`${item.kind}:${item.id}`}
                    label={`${item.name} — ${attention(item)}`}
                  >
                    <Link
                      className={`moment-dashboard-row moment-attention-row ${warning ? 'is-warning' : 'is-failure'}`}
                      to={item.kind === 'runner' ? '/runners' : '/runs'}
                      search={item.kind === 'runner' ? { runner: item.id } : { run: item.id }}
                      aria-label={`${item.name}，${attention(item)}`}
                    >
                      <i className="moment-attention-dot" aria-hidden="true" />
                      <span className="moment-attention-title">{item.name}</span>
                      <span className="moment-attention-detail">{detail}</span>
                    </Link>
                  </TooltipTarget>
                )
              })
            ) : (
              <p className="moment-dashboard-empty">{t('moments.noAttention')}</p>
            )}
          </section>
          <section className="moment-dashboard-section">
            <div className="moment-section-title">
              <h4>{t('moments.upcoming')}</h4>
              <span className="text-muted">{data.timezone}</span>
            </div>
            {data.upcoming.length ? (
              data.upcoming.map((task) => (
                <TooltipTarget
                  key={task.id}
                  label={`${task.name} — ${new Date(task.next_at).toLocaleString(locale(), { timeZone: data.timezone, hourCycle: 'h23' })} ${data.timezone}`}
                >
                  <Link className="moment-dashboard-row" to="/tasks" search={{ task: task.id }}>
                    <span>{task.name}</span>
                    <time dateTime={task.next_at}>{scheduled(task.next_at)}</time>
                  </Link>
                </TooltipTarget>
              ))
            ) : (
              <p className="moment-dashboard-empty">{t('moments.noUpcoming')}</p>
            )}
          </section>
        </>
      )}
    </aside>
  )
}
