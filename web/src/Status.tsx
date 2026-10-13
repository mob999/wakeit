import { stateNames } from './api'
export function Status({ state }: { state: string }) {
  return (
    <span className={`status ${state}`}>
      <i />
      {state === 'not_run' ? '未执行' : (stateNames[state] ?? state)}
    </span>
  )
}
