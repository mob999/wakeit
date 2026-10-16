import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { type Runner } from '../api'
export function RunnerStatus({ runner }: { runner: Runner }) {
  useTranslation()
  return (
    <span
      className={`status ${runner.revoked ? 'cancelled' : runner.online ? 'succeeded' : 'queued'}`}
    >
      <i />
      {runner.revoked ? tr('已吊销') : runner.online ? tr('在线') : tr('离线')}
    </span>
  )
}
