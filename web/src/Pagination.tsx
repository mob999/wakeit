import { ActionIcon } from './actions/ActionIcon'
import { t as tr } from './i18n'
import { useTranslation } from 'react-i18next'

import { IconButton } from './ui'
export function Pagination({
  page,
  total,
  size = 20,
  onChange,
  pending = false,
}: {
  page: number
  total: number
  size?: number
  onChange: (page: number) => void
  pending?: boolean
}) {
  useTranslation()
  const pages = Math.max(1, Math.ceil(total / size))
  return (
    <nav className="pagination" aria-label={tr('分页')}>
      <span>{tr('pagination.total', { count: total })}</span>
      <div>
        <IconButton
          label={tr('上一页')}
          disabled={page <= 1 || pending}
          onClick={() => onChange(page - 1)}
        >
          <ActionIcon name="previous" />
        </IconButton>
        <span>
          {page} / {pages}
        </span>
        <IconButton
          label={tr('下一页')}
          disabled={page >= pages || pending}
          onClick={() => onChange(page + 1)}
        >
          <ActionIcon name="next" />
        </IconButton>
      </div>
    </nav>
  )
}
