import { RunnerStatus } from './RunnerStatus'
import { ContentCard } from '../ContentCard'
import { DetailBody } from '../Workbench'
import { time, type Runner } from '../api'
export function RunnerOverview({ runner }: { runner: Runner }) {
  return (
    <DetailBody limited>
      <ContentCard title="设备信息">
        <dl className="properties runner-properties">
          <dt>状态</dt>
          <dd>
            <RunnerStatus runner={runner} />
          </dd>
          <dt>系统</dt>
          <dd>{[runner.info.os, runner.info.arch].filter(Boolean).join(' / ') || '—'}</dd>
          <dt>Wakeit</dt>
          <dd className="mono">{runner.info.version ?? '—'}</dd>
          <dt>并发占用</dt>
          <dd>
            {(runner.info.active ?? 0) + (runner.info.unknown ?? 0)} /{' '}
            {runner.info.concurrency ?? '—'}
            {runner.info.unknown ? (
              <span className="quiet"> · {runner.info.unknown} 个待核对</span>
            ) : null}
          </dd>
          <dt>最近连接</dt>
          <dd>{time(runner.last_seen)}</dd>
          <dt>执行范围</dt>
          <dd>
            {runner.directory_roots.length ? (
              runner.directory_roots.map((root) => (
                <div key={root} className="mono path-value">
                  {root}
                </div>
              ))
            ) : (
              <span className="quiet">未配置</span>
            )}
          </dd>
        </dl>
      </ContentCard>
    </DetailBody>
  )
}
