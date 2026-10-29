import { ErrorToast } from './Toast'
import { useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { RefreshCw } from './icons'
import { api } from './api'
import { t } from './i18n'
import { Button, Modal, TooltipTarget } from './ui'
import { MarkdownContent } from './runs/MarkdownContent'
import { LanguageSwitch } from './LanguageSwitch'
import englishChangelog from '../../CHANGELOG.md?raw'
import chineseChangelog from '../../CHANGELOG.zh-CN.md?raw'

type ChangelogDocument = { version: string; content: string }

export function Changelog({ version }: { version?: string }) {
  const { i18n } = useTranslation()
  const language = i18n.language === 'zh' ? 'zh' : 'en'
  const localContent = import.meta.env.DEV
    ? language === 'zh'
      ? chineseChangelog
      : englishChangelog
    : undefined
  const [open, setOpen] = useState(false)
  const trigger = useRef<HTMLButtonElement>(null)
  const document = useQuery<ChangelogDocument>({
    queryKey: ['changelog', version, language],
    queryFn: () => api(`/changelog?lang=${language}`),
    enabled: open && localContent === undefined,
    staleTime: Infinity,
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
  })
  return (
    <>
      <TooltipTarget label={t('更新日志')}>
        <button
          ref={trigger}
          type="button"
          className="brand-version changelog-trigger"
          aria-label={t('更新日志 {{version}}', { version: version ?? '' })}
          aria-haspopup="dialog"
          disabled={!version}
          onClick={() => setOpen(true)}
        >
          {version ?? '…'}
        </button>
      </TooltipTarget>
      <Modal
        open={open}
        onOpenChange={setOpen}
        onOpenAutoFocus={(event) => {
          event.preventDefault()
          if (event.target instanceof HTMLElement) event.target.focus()
        }}
        onCloseAutoFocus={(event) => {
          event.preventDefault()
          trigger.current?.focus()
        }}
        title={t('更新日志')}
        actions={<LanguageSwitch />}
      >
        <div className="modal-body changelog-body" aria-busy={document.isFetching || undefined}>
          {localContent !== undefined ? (
            <div className="markdown-result">
              <MarkdownContent text={localContent} />
            </div>
          ) : document.isPending ? (
            <p role="status">{t('正在加载…')}</p>
          ) : document.isError ? (
            <ErrorToast
              error={t('无法加载更新日志')}
              actions={
                <Button loading={document.isFetching} onClick={() => void document.refetch()}>
                  <RefreshCw size={14} />
                  {t('重试')}
                </Button>
              }
            />
          ) : (
            <div className="markdown-result">
              <MarkdownContent text={document.data.content} />
            </div>
          )}
        </div>
      </Modal>
    </>
  )
}
