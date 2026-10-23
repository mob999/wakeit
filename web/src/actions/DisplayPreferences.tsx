import { useTranslation } from 'react-i18next'
import { setLanguage, t } from '../i18n'
import { ActionMenu, ActionMenuItem } from './ActionMenu'
export function DisplayPreferences({ dark, onTheme }: { dark: boolean; onTheme: () => void }) {
  const { i18n } = useTranslation()
  return (
    <ActionMenu label={t('actions.displaySettings')} icon="settings">
      <ActionMenuItem icon={dark ? 'light' : 'dark'} onSelect={onTheme}>
        {t(dark ? 'actions.toLight' : 'actions.toDark')}
      </ActionMenuItem>
      <ActionMenuItem
        icon="language"
        onSelect={() => void setLanguage(i18n.language === 'zh' ? 'en' : 'zh')}
      >
        {t(i18n.language === 'zh' ? '切换到英文' : '切换到中文')}
      </ActionMenuItem>
    </ActionMenu>
  )
}
