import { ErrorToast } from '../Toast'
import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import * as Dropdown from '@radix-ui/react-dropdown-menu'
import { MoreHorizontal, Plus } from 'lucide-react'
import { api, type Member, type User } from '../api'
import { t as tr } from '../i18n'
import { Button, IconButton, Modal } from '../ui'
import { LanguageSwitch } from '../LanguageSwitch'
import { Pagination } from '../Pagination'
import { type PageResult } from '../Search'
import { AccountDialog } from './AccountDialog'

function AccountRow({
  account,
  self,
  activeAdmins,
  local,
}: {
  account: Member
  self: boolean
  activeAdmins: number
  local: boolean
}) {
  useTranslation()
  const client = useQueryClient()
  const [password, setPassword] = useState(false)
  const [confirm, setConfirm] = useState(false)
  const change = useMutation({
    mutationFn: () => api(`/instance/users/${account.id}`, 'PUT', { active: !account.active }),
    onSuccess: () => {
      setConfirm(false)
      client.invalidateQueries({ queryKey: ['instance-users'] })
      if (self) location.assign('/')
    },
  })
  return (
    <>
      <div className="settings-member-row">
        <div className="settings-member-name">
          <span>{account.username}</span>
        </div>
        <span className="quiet">{account.instance_admin ? tr('实例管理员') : '—'}</span>
        <span className="quiet">{account.active ? tr('启用') : tr('已停用')}</span>
        <Dropdown.Root>
          <Dropdown.Trigger asChild>
            <IconButton label={tr('{{v0}} 操作', { v0: account.username })}>
              <MoreHorizontal />
            </IconButton>
          </Dropdown.Trigger>
          <Dropdown.Portal>
            <Dropdown.Content className="dropdown" align="end">
              <Dropdown.Item
                disabled={account.active && account.instance_admin && activeAdmins === 1}
                onSelect={() => {
                  change.reset()
                  setConfirm(true)
                }}
              >
                {account.active ? tr('停用账号') : tr('启用账号')}
              </Dropdown.Item>
              {local && account.has_password !== false && (
                <Dropdown.Item onSelect={() => setPassword(true)}>{tr('重置密码')}</Dropdown.Item>
              )}
            </Dropdown.Content>
          </Dropdown.Portal>
        </Dropdown.Root>
      </div>
      {password && (
        <AccountDialog member={account} self={self} onClose={() => setPassword(false)} />
      )}
      <Modal
        open={confirm}
        onOpenChange={(open) => !change.isPending && setConfirm(open)}
        title={account.active ? tr('停用账号') : tr('启用账号')}
        description={
          account.active
            ? tr('停用「{{v0}}」后，全部登录和 API Key 将失效。{{v1}}', {
                v0: account.username,
                v1: self ? tr('你将退出登录。') : '',
              })
            : tr('启用「{{v0}}」？原有登录和 API Key 不会恢复。', { v0: account.username })
        }
      >
        <div className="modal-body">
          {change.error && <ErrorToast>{change.error.message}</ErrorToast>}
        </div>
        <div className="modal-footer">
          <Button disabled={change.isPending} onClick={() => setConfirm(false)}>
            {tr('取消')}
          </Button>
          <Button variant="solid" disabled={change.isPending} onClick={() => change.mutate()}>
            {tr('确认')}
          </Button>
        </div>
      </Modal>
    </>
  )
}

export function InstanceAccounts({ user }: { user: User }) {
  useTranslation()
  const [page, setPage] = useState(1)
  const [create, setCreate] = useState(false)
  const query = useQuery<PageResult<Member> & { active_admins: number }>({
    queryKey: ['instance-users', page],
    queryFn: () => api(`/instance/users?page=${page}&page_size=20`),
  })
  const meta = useQuery<{ auth?: { mode: string } }>({
    queryKey: ['meta'],
    queryFn: () => api('/meta'),
  })
  const local = meta.data?.auth?.mode !== 'github'
  return (
    <div className="workspace-home">
      <header className="workspace-home-header">
        <nav className="breadcrumbs">
          <a href="/">Wakeit</a>
          <span>/</span>
          <span>{tr('实例账号管理')}</span>
        </nav>
        <div className="top-actions">
          <LanguageSwitch />
          {local && (
            <IconButton label={tr('新建账号')} onClick={() => setCreate(true)}>
              <Plus />
            </IconButton>
          )}
        </div>
      </header>
      <main className="instance-accounts-body">
        {query.error ? (
          <div className="content-card card-body">
            <ErrorToast>{query.error.message}</ErrorToast>
            <Button onClick={() => void query.refetch()}>{tr('重试')}</Button>
          </div>
        ) : query.isPending ? (
          <div className="quiet">{tr('加载中…')}</div>
        ) : (
          <>
            <div className="content-card settings-table settings-table-members">
              <div className="settings-member-row settings-table-head table-head">
                <span>{tr('账号')}</span>
                <span>{tr('权限')}</span>
                <span>{tr('状态')}</span>
                <span>{tr('操作')}</span>
              </div>
              {query.data.items.map((account) => (
                <AccountRow
                  key={account.id}
                  account={account}
                  self={account.id === user.id}
                  activeAdmins={query.data.active_admins}
                  local={local}
                />
              ))}
            </div>
            <Pagination
              page={page}
              total={query.data.total}
              pending={query.isFetching}
              onChange={setPage}
            />
          </>
        )}
      </main>
      {create && <AccountDialog onClose={() => setCreate(false)} />}
    </div>
  )
}
