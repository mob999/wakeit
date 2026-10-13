import { PageActions } from '../PageHeader'
import { useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import * as Dropdown from '@radix-ui/react-dropdown-menu'
import { ChevronDown, UserPlus, RefreshCw, Plus, MoreHorizontal } from 'lucide-react'
import { api, type Member, type User } from '../api'
import { type PageResult } from '../Search'
import { Pagination } from '../Pagination'
import { DetailBody } from '../Workbench'
import { Button, IconButton, Field, Modal } from '../ui'
import { AccountDialog } from './AccountDialog'
import { RequestState, SecretValue } from './shared'
const roles = ['admin', 'operator', 'viewer'] as const
const roleName = (role: string) => role[0].toUpperCase() + role.slice(1)
function MemberRow({
  member,
  activeAdmins,
  user,
}: {
  member: Member
  activeAdmins: number
  user: User
}) {
  const client = useQueryClient()
  const [account, setAccount] = useState(false)
  const [activation, setActivation] = useState(false)
  const activate = useMutation({
    mutationFn: () => api(`/users/${member.id}`, 'PUT', { active: !member.active }),
    onSuccess: () => {
      if (member.id === user.id) {
        client.clear()
        location.assign('/')
        return
      }
      client.invalidateQueries({ queryKey: ['members'] })
      setActivation(false)
    },
  })
  const [proposed, setProposed] = useState<string | null>(null)
  const lastAdmin = member.active && member.role === 'admin' && activeAdmins === 1
  const change = useMutation({
    mutationFn: (role: string) => api(`/users/${member.id}`, 'PUT', { role }),
    onSuccess: async () => {
      await client.invalidateQueries({ queryKey: ['members'] })
      if (member.id === user.id) await client.invalidateQueries({ queryKey: ['me'] })
      setProposed(null)
    },
  })
  const select = (role: string) => {
    if (role === member.role) return
    if (member.id === user.id && member.role === 'admin' && role !== 'admin') setProposed(role)
    else change.mutate(role)
  }
  return (
    <>
      <div className="settings-member-row">
        <div className="settings-member-name">
          <span className="avatar">{member.username[0].toUpperCase()}</span>
          <span>{member.username}</span>
        </div>
        <div className="settings-member-role">
          <Dropdown.Root>
            <Dropdown.Trigger asChild>
              <button
                className="settings-role-button"
                aria-label={`${member.username} 角色`}
                disabled={change.isPending}
              >
                {roleName(member.role)}
                {change.isPending ? (
                  <RefreshCw size={12} className="spinning" />
                ) : (
                  <ChevronDown size={12} />
                )}
              </button>
            </Dropdown.Trigger>
            <Dropdown.Portal>
              <Dropdown.Content className="dropdown settings-role-menu" align="end">
                <Dropdown.RadioGroup value={member.role} onValueChange={select}>
                  {roles.map((role) => (
                    <Dropdown.RadioItem
                      key={role}
                      value={role}
                      disabled={lastAdmin && role !== 'admin'}
                    >
                      {roleName(role)}
                    </Dropdown.RadioItem>
                  ))}
                </Dropdown.RadioGroup>
                {lastAdmin && <div className="settings-role-help">至少保留一名管理员</div>}
              </Dropdown.Content>
            </Dropdown.Portal>
          </Dropdown.Root>
          {change.error && (
            <small className="error" role="alert">
              {change.error.message}
            </small>
          )}
        </div>
        <span className="quiet">{member.active ? '启用' : '已停用'}</span>
        <Dropdown.Root>
          <Dropdown.Trigger asChild>
            <IconButton label={`${member.username} 操作`} disabled={change.isPending}>
              <MoreHorizontal />
            </IconButton>
          </Dropdown.Trigger>
          <Dropdown.Portal>
            <Dropdown.Content className="dropdown" align="end">
              <Dropdown.Item
                disabled={lastAdmin}
                onSelect={() => {
                  activate.reset()
                  setActivation(true)
                }}
              >
                {member.active ? '停用账号' : '启用账号'}
              </Dropdown.Item>
              <Dropdown.Item onSelect={() => setAccount(true)}>重置密码</Dropdown.Item>
            </Dropdown.Content>
          </Dropdown.Portal>
        </Dropdown.Root>
      </div>
      {account && (
        <AccountDialog
          member={member}
          self={member.id === user.id}
          onClose={() => setAccount(false)}
        />
      )}
      <Modal
        open={activation}
        onOpenChange={(v) => {
          if (!v && !activate.isPending) setActivation(false)
        }}
        title={member.active ? '停用账号' : '启用账号'}
        description={
          member.active
            ? `停用「${member.username}」后，全部登录和 API Key 将失效。${member.id === user.id ? '你将退出登录。' : ''}`
            : `启用「${member.username}」？原有登录和 API Key 不会恢复。`
        }
      >
        <div className="modal-body">
          {activate.error && (
            <p className="error" role="alert">
              {activate.error.message}
            </p>
          )}
        </div>
        <div className="modal-footer">
          <Button disabled={activate.isPending} onClick={() => setActivation(false)}>
            取消
          </Button>
          <Button
            variant="solid"
            danger={member.active}
            disabled={activate.isPending}
            onClick={() => activate.mutate()}
          >
            {activate.isPending ? '正在保存…' : member.active ? '停用' : '启用'}
          </Button>
        </div>
      </Modal>
      <Modal
        open={!!proposed}
        onOpenChange={(v) => {
          if (!v && !change.isPending) setProposed(null)
        }}
        title="更改自己的角色"
        description={`降为 ${roleName(proposed ?? 'operator')} 后将无法管理成员和工作区。`}
      >
        <div className="modal-body">
          {change.error && (
            <p className="error" role="alert">
              {change.error.message}
            </p>
          )}
        </div>
        <div className="modal-footer">
          <Button disabled={change.isPending} onClick={() => setProposed(null)}>
            取消
          </Button>
          <Button
            variant="solid"
            disabled={change.isPending}
            onClick={() => proposed && change.mutate(proposed)}
          >
            确认更改
          </Button>
        </div>
      </Modal>
    </>
  )
}
function InviteDialog({ onClose }: { onClose: () => void }) {
  const [role, setRole] = useState('operator')
  const [link, setLink] = useState('')
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
    if (busy.current || link) return
    busy.current = true
    setPending(true)
    setError('')
    try {
      const result = await api<{ url: string }>('/invites', 'POST', { role })
      if (alive.current) setLink(result.url)
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
      title="邀请成员"
    >
      <form
        onSubmit={(e) => {
          e.preventDefault()
          create()
        }}
      >
        <div className="modal-body">
          {link ? (
            <>
              <SecretValue label="邀请链接" value={link} />
              <p className="quiet">一次性链接，48 小时内有效</p>
            </>
          ) : (
            <Field label="邀请角色">
              <select value={role} disabled={pending} onChange={(e) => setRole(e.target.value)}>
                {roles.map((role) => (
                  <option key={role} value={role}>
                    {roleName(role)}
                  </option>
                ))}
              </select>
            </Field>
          )}
          {error && (
            <p className="error" role="alert">
              {error}
            </p>
          )}
        </div>
        <div className="modal-footer">
          <Button type="button" disabled={pending} onClick={onClose}>
            {link ? '关闭' : '取消'}
          </Button>
          {!link && (
            <Button type="submit" variant="solid" disabled={pending}>
              {pending ? '正在生成…' : '生成链接'}
            </Button>
          )}
        </div>
      </form>
    </Modal>
  )
}
export function MembersSettings({ user }: { user: User }) {
  const [page, setPage] = useState(1)
  const [invite, setInvite] = useState(false)
  const [create, setCreate] = useState(false)
  const query = useQuery<PageResult<Member> & { active_admins: number }>({
    queryKey: ['members', page],
    queryFn: () => api(`/users?page=${page}&page_size=20`),
    refetchInterval: 3000,
  })
  useEffect(() => {
    if (query.data && (page - 1) * 20 >= query.data.total && page > 1)
      setPage(Math.max(1, Math.ceil(query.data.total / 20)))
  }, [query.data, page])
  return (
    <>
      <PageActions>
        <IconButton label="新建账号" onClick={() => setCreate(true)}>
          <Plus />
        </IconButton>
        <IconButton label="邀请成员" onClick={() => setInvite(true)}>
          <UserPlus />
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
          <div className="settings-request content-card">暂无成员</div>
        ) : (
          <div className="settings-table content-card">
            <div className="settings-member-row settings-table-head table-head">
              <span>成员</span>
              <span>角色</span>
              <span>状态</span>
              <span>操作</span>
            </div>
            {query.data.items.map((member) => (
              <MemberRow
                key={member.id}
                member={member}
                activeAdmins={query.data.active_admins}
                user={user}
              />
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
      {create && (
        <AccountDialog
          onClose={() => setCreate(false)}
          onCreated={() => setPage(Math.max(1, Math.ceil(((query.data?.total ?? 0) + 1) / 20)))}
        />
      )}
      {invite && <InviteDialog onClose={() => setInvite(false)} />}
    </>
  )
}
