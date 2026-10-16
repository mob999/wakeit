import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import YAML from 'yaml'
import { type Agent, type Run, type RunEvent, time } from '../api'
import { Status } from '../Status'
import { DetailBody } from '../Workbench'
import { RunIdentity } from './RunListRow'
export function RunDetails({
  run: r,
  agents,
  events,
}: {
  run: Run
  agents?: Agent[]
  events: RunEvent[]
}) {
  useTranslation()
  const parameters = r.execution_snapshot?.parameters ?? r.config.parameters ?? {}
  return (
    <DetailBody limited>
      <dl className="properties run-properties">
        <dt>{tr('执行设备')}</dt>
        <dd>
          <RunIdentity run={r} agents={agents} />
        </dd>
        <dt>{tr('工作目录')}</dt>
        <dd className="mono wrap">{r.execution_snapshot?.cwd ?? r.config.cwd}</dd>
        <dt>{tr('模型')}</dt>
        <dd className="mono">
          {r.execution_snapshot?.model ?? parameters.model ?? tr('Agent 默认')}
        </dd>
        <dt>{tr('参数')}</dt>
        <dd>
          {Object.keys(parameters).length ? (
            <div className="run-parameters">
              {Object.entries(parameters).map(([key, value]) => (
                <div key={key}>
                  <span className="quiet mono">{key}</span>
                  <span className="mono wrap">{value}</span>
                </div>
              ))}
            </div>
          ) : (
            <span className="quiet">{tr('默认配置')}</span>
          )}
        </dd>
        {r.session && (
          <>
            {r.session.title?.trim() && (
              <>
                <dt>{tr('会话名称')}</dt>
                <dd className="wrap">{r.session.title}</dd>
              </>
            )}
            <dt>{tr('会话 ID')}</dt>
            <dd className="mono wrap">{r.session.id}</dd>
            {!r.session.ready && r.session.reason && (
              <>
                <dt>{tr('会话状态')}</dt>
                <dd>{r.session.reason}</dd>
              </>
            )}
          </>
        )}
        <dt>{tr('计划时间')}</dt>
        <dd>{time(r.scheduled_at)}</dd>
        <dt>{tr('开始时间')}</dt>
        <dd>{time(r.started_at)}</dd>
        <dt>{tr('结束时间')}</dt>
        <dd>{time(r.finished_at)}</dd>
        <dt>{tr('来源')}</dt>
        <dd>
          {{ manual: tr('手动'), scheduled: tr('定时'), schedule: tr('定时'), retry: tr('重试') }[
            r.source
          ] ?? r.source}
        </dd>
        {r.reason && (
          <>
            <dt>{tr('原因')}</dt>
            <dd className="wrap">{r.reason}</dd>
          </>
        )}
        <dt>{tr('Run ID')}</dt>
        <dd className="mono wrap">{r.id}</dd>
        <dt>{tr('Task ID')}</dt>
        <dd className="mono wrap">{r.task_id}</dd>
        {r.job_id && (
          <>
            <dt>{tr('外部任务 ID')}</dt>
            <dd className="mono wrap">{r.job_id}</dd>
          </>
        )}
      </dl>
      <details className="run-details-fold">
        <summary>{tr('任务配置')}</summary>
        <pre className="code">{YAML.stringify(r.config)}</pre>
      </details>
      <details className="run-details-fold">
        <summary>{tr('状态事件')}</summary>
        {events
          .filter((e) => e.kind === 'state')
          .map((e) => (
            <div className="timeline-row" key={e.id}>
              <span className="quiet mono">{time(e.created_at)}</span>
              <Status state={e.data.state ?? ''} />
            </div>
          ))}
      </details>
    </DetailBody>
  )
}
