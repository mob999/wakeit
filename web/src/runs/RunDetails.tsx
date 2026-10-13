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
  const parameters = r.execution_snapshot?.parameters ?? r.config.parameters ?? {}
  return (
    <DetailBody limited>
      <dl className="properties run-properties">
        <dt>执行设备</dt>
        <dd>
          <RunIdentity run={r} agents={agents} />
        </dd>
        <dt>工作目录</dt>
        <dd className="mono wrap">{r.execution_snapshot?.cwd ?? r.config.cwd}</dd>
        <dt>模型</dt>
        <dd className="mono">{r.execution_snapshot?.model ?? parameters.model ?? 'Agent 默认'}</dd>
        <dt>参数</dt>
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
            <span className="quiet">默认配置</span>
          )}
        </dd>
        {r.session && (
          <>
            {r.session.title?.trim() && (
              <>
                <dt>会话名称</dt>
                <dd className="wrap">{r.session.title}</dd>
              </>
            )}
            <dt>会话 ID</dt>
            <dd className="mono wrap">{r.session.id}</dd>
            {!r.session.ready && r.session.reason && (
              <>
                <dt>会话状态</dt>
                <dd>{r.session.reason}</dd>
              </>
            )}
          </>
        )}
        <dt>计划时间</dt>
        <dd>{time(r.scheduled_at)}</dd>
        <dt>开始时间</dt>
        <dd>{time(r.started_at)}</dd>
        <dt>结束时间</dt>
        <dd>{time(r.finished_at)}</dd>
        <dt>来源</dt>
        <dd>
          {{ manual: '手动', scheduled: '定时', schedule: '定时', retry: '重试' }[r.source] ??
            r.source}
        </dd>
        {r.reason && (
          <>
            <dt>原因</dt>
            <dd className="wrap">{r.reason}</dd>
          </>
        )}
        <dt>Run ID</dt>
        <dd className="mono wrap">{r.id}</dd>
        <dt>Task ID</dt>
        <dd className="mono wrap">{r.task_id}</dd>
        {r.job_id && (
          <>
            <dt>外部任务 ID</dt>
            <dd className="mono wrap">{r.job_id}</dd>
          </>
        )}
      </dl>
      <details className="run-details-fold">
        <summary>任务配置</summary>
        <pre className="code">{YAML.stringify(r.config)}</pre>
      </details>
      <details className="run-details-fold">
        <summary>状态事件</summary>
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
