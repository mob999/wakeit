import { type Runner } from '../api'
export function RunnerStatus({ runner }: { runner: Runner }) {
  return (
    <span
      className={`status ${runner.revoked ? 'cancelled' : runner.online ? 'succeeded' : 'queued'}`}
    >
      <i />
      {runner.revoked ? '已吊销' : runner.online ? '在线' : '离线'}
    </span>
  )
}
