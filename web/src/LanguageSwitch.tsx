import { Languages } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { setLanguage, t } from './i18n'
import { IconButton } from './ui'

export function LanguageSwitch({ className }: { className?: string }) {
  const { i18n } = useTranslation()
  const next = i18n.language === 'zh' ? 'en' : 'zh'
  return (
    <IconButton
      label={next === 'en' ? t('切换到英文') : t('切换到中文')}
      className={className}
      onClick={() => void setLanguage(next)}
    >
      <Languages />
    </IconButton>
  )
}
