import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { api, type Run, type Agent, type Runner, time } from '../api'
import { t as tr } from '../i18n'
import { Status } from '../Status'
import { ChevronRight } from '../icons'
import { ErrorToast } from '../Toast'
import { RunIdentity, RunDevice, RunAgent } from './RunListRow'
export function RunTable({
  runs,
  title,
  agents,
  embedded = false,
  actions,
  pending = false,
  error,
}: {
  runs: Run[]
  title: string
  agents?: Agent[]
  embedded?: boolean
  actions?: ReactNode
  pending?: boolean
  error?: Error | null
}) {
  useTranslation()
  const runners = useQuery<Runner[]>({ queryKey: ['runners'], queryFn: () => api('/runners') })
  return (
    <section className={'run-table ' + (embedded ? 'task-overview-history' : 'content-card')}>
      <div className="run-table-head run-table-row table-head">
        <h3>{title}</h3>
        <span>{tr('执行设备')}</span>
        <span>{tr('Agent')}</span>
        <span>{tr('执行时间')}</span>
        <span className="run-table-actions">{actions}</span>
      </div>
      {error ? (
        <ErrorToast>{error.message}</ErrorToast>
      ) : pending ? (
        <p className="quiet card-state">{tr('正在加载…')}</p>
      ) : runs.length ? (
        <>
          {runs.map((r) => (
            <Link to="/runs" search={{ run: r.id }} key={r.id} className="run-table-row">
              <Status state={r.state} />
              {r.config.dag ? (
                <span className="dag-table-identity">
                  <RunIdentity run={r} />
                </span>
              ) : (
                <>
                  <RunDevice run={r} agents={agents} runners={runners.data} />
                  <RunAgent run={r} agents={agents} />
                </>
              )}
              <span className="run-table-time">{time(r.started_at ?? r.created_at)}</span>
              <ChevronRight size={13} />
            </Link>
          ))}
        </>
      ) : (
        <p className="quiet card-state">{tr('暂无执行记录')}</p>
      )}
    </section>
  )
}
