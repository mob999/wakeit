import { t as tr } from './i18n'
import { useTranslation } from 'react-i18next'
import { stateNames } from './api'
export function Status({ state }: { state: string }) {
  useTranslation()
  return (
    <span className={`status ${state}`}>
      <i />
      {state === 'not_run' ? tr('未执行') : (stateNames[state] ?? state)}
    </span>
  )
}
