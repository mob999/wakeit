import { t as tr, locale } from '../i18n'
import { useTranslation } from 'react-i18next'
import { PageActions } from '../PageHeader'
import { useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Plus, Ban } from 'lucide-react'
import { api, type ApiKey } from '../api'
import { type PageResult } from '../Search'
import { Pagination } from '../Pagination'
import { DetailBody } from '../Workbench'
import { Button, IconButton, Field, Modal } from '../ui'
import { ChoicePicker } from '../ChoicePicker'
import { RequestState, SecretValue } from './shared'
function CreateTokenDialog({ onClose }: { onClose: () => void }) {
  useTranslation()
  const client = useQueryClient()
  const [name, setName] = useState('')
  const [expiration, setExpiration] = useState('never')
  const [customDate, setCustomDate] = useState('')
  const [secret, setSecret] = useState('')
  const [pending, setPending] = useState(false)
  const [error, setError] = useState('')
  const busy = useRef(false)
  const alive = useRef(true)
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
    }
  }, [])

  const create = async () => {
    if (busy.current || secret || !name.trim()) return
    setError('')
    const expiresAt =
      expiration === 'never'
        ? null
        : expiration === 'custom'
          ? new Date(customDate)
          : new Date(Date.now() + Number(expiration) * 86400000)
    if (expiresAt && (!Number.isFinite(expiresAt.getTime()) || expiresAt.getTime() <= Date.now())) {
      setError(tr('请选择未来的过期时间'))
      return
    }
    busy.current = true
    setPending(true)
    try {
      const result = await api<{ token: string }>('/tokens', 'POST', {
        name: name.trim(),
        expires_at: expiresAt?.toISOString() ?? null,
      })
      if (alive.current) setSecret(result.token)
      client.invalidateQueries({ queryKey: ['tokens'] })
    } catch (e) {
      if (alive.current) setError((e as Error).message)
    } finally {
      busy.current = false
      if (alive.current) setPending(false)
    }
  }
  return (
    <Modal
      open
      onOpenChange={(v) => {
        if (!v && !busy.current) onClose()
      }}
      title={tr('创建 API Key')}
    >
      <form
        onSubmit={(e) => {
          e.preventDefault()
          create()
        }}
      >
        <div className="modal-body">
          {secret ? (
            <>
              <SecretValue label={tr('API Key')} value={secret} />
              <p className="quiet">{tr('仅显示一次，请保存')}</p>
            </>
          ) : (
            <>
              <Field label={tr('名称')}>
                <input
                  autoFocus
                  value={name}
                  disabled={pending}
                  required
                  onChange={(e) => setName(e.target.value)}
                />
              </Field>
              <Field label={tr('过期时间')}>
                <ChoicePicker
                  label={tr('过期时间')}
                  value={expiration}
                  disabled={pending}
                  onChange={setExpiration}
                  options={[
                    { value: 'never', label: tr('不过期') },
                    { value: '7', label: tr('7 天') },
                    { value: '30', label: tr('30 天') },
                    { value: '90', label: tr('90 天') },
                    { value: '365', label: tr('1 年') },
                    { value: 'custom', label: tr('自定义时间') },
                  ]}
                />
              </Field>
              {expiration === 'custom' && (
                <Field label={tr('到期时间')}>
                  <input
                    type="datetime-local"
                    required
                    value={customDate}
                    disabled={pending}
                    onChange={(e) => setCustomDate(e.target.value)}
                  />
                </Field>
              )}
            </>
          )}
          {error && (
            <p className="error" role="alert">
              {error}
            </p>
          )}
        </div>
        <div className="modal-footer">
          <Button type="button" disabled={pending} onClick={onClose}>
            {secret ? tr('关闭') : tr('取消')}
          </Button>
          {!secret && (
            <Button type="submit" variant="solid" disabled={pending || !name.trim()}>
              {pending ? tr('正在创建…') : tr('创建')}
            </Button>
          )}
        </div>
      </form>
    </Modal>
  )
}
export function TokenSettings() {
  useTranslation()
  const client = useQueryClient()
  const [page, setPage] = useState(1)
  const [create, setCreate] = useState(false)
  const [selected, setSelected] = useState<ApiKey | null>(null)
  const query = useQuery<PageResult<ApiKey>>({
    queryKey: ['tokens', page],
    queryFn: () => api(`/tokens?page=${page}&page_size=20`),
  })
  useEffect(() => {
    if (query.data && (page - 1) * 20 >= query.data.total && page > 1)
      setPage(Math.max(1, Math.ceil(query.data.total / 20)))
  }, [query.data, page])
  const revoke = useMutation({
    mutationFn: (id: string) => api(`/tokens/${id}`, 'DELETE'),
    onSuccess: () => {
      setSelected(null)
      client.invalidateQueries({ queryKey: ['tokens'] })
    },
  })
  return (
    <>
      <PageActions>
        <IconButton label={tr('创建 API Key')} onClick={() => setCreate(true)}>
          <Plus />
        </IconButton>
      </PageActions>
      <DetailBody limited>
        {query.error || query.isPending ? (
          <RequestState
            pending={query.isPending}
            error={query.error}
            retry={() => query.refetch()}
          />
        ) : !query.data.items.length ? (
          <div className="settings-request content-card">{tr('暂无 API Key')}</div>
        ) : (
          <div className="settings-table content-card">
            <div className="settings-token-row settings-table-head table-head">
              <span>{tr('名称')}</span>
              <span>{tr('到期时间')}</span>
              <span>{tr('操作')}</span>
            </div>
            {query.data.items.map((token) => (
              <div className="settings-token-row" key={token.id}>
                <span className="settings-token-name">
                  {token.name}
                  {token.expires_at && new Date(token.expires_at).getTime() < Date.now() && (
                    <small className="quiet">{tr('已过期')}</small>
                  )}
                </span>
                <span className="mono quiet">
                  {token.expires_at
                    ? new Date(token.expires_at).toLocaleString(locale())
                    : tr('不过期')}
                </span>
                <IconButton
                  label={tr('撤销')}
                  tooltip={tr('撤销 {{v0}}', { v0: token.name })}
                  danger
                  onClick={() => {
                    revoke.reset()
                    setSelected(token)
                  }}
                >
                  <Ban />
                </IconButton>
              </div>
            ))}
          </div>
        )}
        {query.data && (
          <Pagination
            page={page}
            total={query.data.total}
            pending={query.isFetching}
            onChange={setPage}
          />
        )}
      </DetailBody>
      {create && <CreateTokenDialog onClose={() => setCreate(false)} />}
      <Modal
        open={!!selected}
        onOpenChange={(v) => {
          if (!v && !revoke.isPending) setSelected(null)
        }}
        title={tr('撤销 API Key')}
        description={tr('撤销「{{v0}}」后，使用它的 CLI 或脚本将失去访问权限。', {
          v0: selected?.name ?? '',
        })}
      >
        <div className="modal-body">
          {revoke.error && (
            <p className="error" role="alert">
              {revoke.error.message}
            </p>
          )}
        </div>
        <div className="modal-footer">
          <Button disabled={revoke.isPending} onClick={() => setSelected(null)}>
            {tr('取消')}
          </Button>
          <Button
            variant="solid"
            danger
            disabled={revoke.isPending}
            onClick={() => selected && revoke.mutate(selected.id)}
          >
            {revoke.isPending ? tr('正在撤销…') : tr('撤销')}
          </Button>
        </div>
      </Modal>
    </>
  )
}
