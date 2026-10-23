import { t } from '../i18n'
import { announceAction } from './announcements'
export async function copyAction(value: string, label: string, onError: (message: string) => void) {
  try {
    await navigator.clipboard.writeText(value)
    announceAction(t('actions.completed', { action: label }))
    return true
  } catch {
    onError(t('请选中文本后复制'))
    return false
  }
}
