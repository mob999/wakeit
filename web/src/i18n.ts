import i18n from 'i18next'
import { initReactI18next } from 'react-i18next'
import zh from './locales/zh.json'
import en from './locales/en.json'

export type Language = 'zh' | 'en'
const preferenceKey = 'wakeit-language'
const supported = (value: string | null): value is Language => value === 'zh' || value === 'en'
let preferred: string | null = null
try {
  preferred = localStorage.getItem(preferenceKey)
} catch {}
const language = supported(preferred)
  ? preferred
  : navigator.language.startsWith('zh')
    ? 'zh'
    : 'en'

void i18n.use(initReactI18next).init({
  resources: { zh: { translation: zh }, en: { translation: en } },
  lng: language,
  fallbackLng: 'zh',
  supportedLngs: ['zh', 'en'],
  initAsync: false,
  keySeparator: false,
  nsSeparator: false,
  interpolation: { escapeValue: false },
})

function applyLanguage(value: string) {
  document.documentElement.lang = value === 'zh' ? 'zh-CN' : 'en'
  try {
    localStorage.setItem(preferenceKey, value)
  } catch {}
}
applyLanguage(i18n.language)
i18n.on('languageChanged', applyLanguage)

export const t = (key: string, values?: Record<string, unknown>): string =>
  i18n.t(key, values ?? {})
export const locale = () => (i18n.language === 'zh' ? 'zh-CN' : 'en-US')
export const setLanguage = (value: Language) => i18n.changeLanguage(value)
export const roleName = (value: string) =>
  ['admin', 'operator', 'viewer'].includes(value) ? t(`role.${value}`) : value
export const parameterLabel = (value: string) =>
  ['模型', '推理强度', 'Provider'].includes(value) ? t(value) : value
export default i18n

export const time = (value: string | null | undefined, timezone?: string) =>
  value
    ? new Date(value).toLocaleString(locale(), {
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
        hourCycle: 'h23',
        ...(timezone ? { timeZone: timezone } : {}),
      })
    : '—'

export const stateNames: Record<string, string> = {
  get queued() {
    return t('等待执行')
  },
  get starting() {
    return t('启动中')
  },
  get running() {
    return t('运行中')
  },
  get succeeded() {
    return t('成功')
  },
  get warning() {
    return t('dag.warning')
  },
  get waiting() {
    return t('dag.waitingDependencies')
  },
  get failed() {
    return t('失败')
  },
  get cancelled() {
    return t('已取消')
  },
  get timed_out() {
    return t('已超时')
  },
  get skipped() {
    return t('已跳过')
  },
  get unknown() {
    return t('状态待核对')
  },
  get interrupted() {
    return t('已中断')
  },
}
