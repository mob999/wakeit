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
