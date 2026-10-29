import { ActionIcon } from './actions/ActionIcon'
import { ErrorToast } from './Toast'
import { useEffect, useState, type ReactNode } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import * as Dropdown from '@radix-ui/react-dropdown-menu'
import { Building2, Check, ChevronDown, LogOut, ArrowUpRight, Users } from './icons'
import { useTranslation } from 'react-i18next'
import { t as tr, roleName } from './i18n'
import { api, workspaceId, type User, type WorkspaceList } from './api'
import { Button, Field, IconButton, Modal, TooltipTarget } from './ui'
import { BrandMark } from './BrandMark'
import { LanguageSwitch } from './LanguageSwitch'
import { useSearchControls } from './Search'

export function CreateWorkspace({ onClose }: { onClose: () => void }) {
  useTranslation()
  const [name, setName] = useState('')
  const create = useMutation({
    mutationFn: () => api<{ id: string }>('/workspaces', 'POST', { name: name.trim() }),
    onSuccess: (result) => location.assign(`/w/${result.id}/`),
  })
  return (
    <Modal
      open
      title={tr('创建工作区')}
      onOpenChange={(open) => !open && !create.isPending && onClose()}
    >
      <form
        onSubmit={(event) => {
          event.preventDefault()
          if (!create.isPending) create.mutate()
        }}
      >
        <div className="modal-body">
          <Field label={tr('工作区名称')}>
            <input
              autoFocus
              required
              maxLength={100}
              value={name}
              disabled={create.isPending}
              onChange={(event) => setName(event.target.value)}
            />
          </Field>
          {create.error && <ErrorToast error={create.error.message} />}
        </div>
        <div className="modal-footer">
          <Button type="button" disabled={create.isPending} onClick={onClose}>
            {tr('取消')}
          </Button>
          <Button type="submit" variant="solid" disabled={create.isPending || !name.trim()}>
            {tr('创建')}
          </Button>
        </div>
      </form>
    </Modal>
  )
}

export function WorkspaceAccountMenu({ user, onLogout }: { user: User; onLogout: () => void }) {
  useTranslation()
  const client = useQueryClient()
  const controls = useSearchControls()
  const workspaces = useQuery<WorkspaceList>({
    queryKey: ['workspaces'],
    queryFn: () => api('/workspaces'),
  })
  const select = async (id: string) => {
    if (id === workspaceId || !(await controls.requestLeave())) return
    await client.cancelQueries()
    location.assign(`/w/${id}/`)
  }
  return (
    <Dropdown.Root>
      <TooltipTarget label={`${user.username} (${roleName(user.role)})`}>
        <Dropdown.Trigger asChild>
          <button className="user-menu" aria-label={tr('用户菜单')}>
            <span className="user-name">{user.username}</span>
            <ChevronDown size={12} aria-hidden="true" />
          </button>
        </Dropdown.Trigger>
      </TooltipTarget>
      <Dropdown.Portal>
        <Dropdown.Content
          className="dropdown workspace-menu"
          align="end"
          sideOffset={6}
          collisionPadding={12}
        >
          <Dropdown.Label className="user-menu-label">{user.username}</Dropdown.Label>
          <Dropdown.Separator className="workspace-menu-separator" />
          <Dropdown.Label className="workspace-menu-label">{tr('工作区')}</Dropdown.Label>
          <Dropdown.RadioGroup
            className="workspace-menu-list"
            aria-label={tr('工作区')}
            value={workspaceId}
            onValueChange={(id) => void select(id)}
          >
            {workspaces.isPending && <Dropdown.Item disabled>{tr('加载中…')}</Dropdown.Item>}
            {workspaces.error && (
              <Dropdown.Item onSelect={() => void workspaces.refetch()}>{tr('重试')}</Dropdown.Item>
            )}
            {[
              ...(workspaces.data?.items ?? []),
              ...(workspaces.data?.demo ? [workspaces.data.demo] : []),
            ].map((workspace) => (
              <Dropdown.RadioItem
                key={workspace.id}
                value={workspace.id}
                aria-label={workspace.name}
                className="workspace-option"
              >
                <span className="workspace-option-name" title={workspace.name}>
                  {workspace.name}
                </span>
                <span className="workspace-option-meta">
                  {workspace.id === workspaceId && <small>{roleName(workspace.role)}</small>}
                  <Dropdown.ItemIndicator className="workspace-check">
                    <Check size={14} aria-hidden="true" />
                  </Dropdown.ItemIndicator>
                </span>
              </Dropdown.RadioItem>
            ))}
          </Dropdown.RadioGroup>
          <Dropdown.Separator className="workspace-menu-separator" />
          {user.instance_admin && (
            <Dropdown.Item
              onSelect={() =>
                controls.requestLeave().then((allowed) => allowed && location.assign('/instance'))
              }
            >
              <Users size={14} aria-hidden="true" />
              {tr('账号管理')}
            </Dropdown.Item>
          )}
          <Dropdown.Item
            className="workspace-signout"
            onSelect={() => controls.requestLeave().then((allowed) => allowed && onLogout())}
          >
            <LogOut size={14} aria-hidden="true" />
            {tr('退出登录')}
          </Dropdown.Item>
        </Dropdown.Content>
      </Dropdown.Portal>
    </Dropdown.Root>
  )
}

export function WorkspaceHome({ user, error }: { user: User; error?: string }) {
  useTranslation()
  const [create, setCreate] = useState(false)
  const [stay, setStay] = useState(!!error)
  const workspaces = useQuery<WorkspaceList>({
    queryKey: ['workspaces'],
    queryFn: () => api('/workspaces'),
  })
  useEffect(() => {
    if (stay || !workspaces.data) return
    const legacy = ['tasks', 'runs', 'runners', 'settings'].includes(location.pathname.slice(1))
    const target =
      location.pathname === '/demo'
        ? workspaces.data.demo
        : legacy
          ? workspaces.data.items.find((item) => item.is_default)
          : workspaces.data.items.length === 1
            ? workspaces.data.items[0]
            : undefined
    if (target)
      location.replace(`/w/${target.id}${legacy ? location.pathname : '/'}${location.search}`)
  }, [workspaces.data, stay])
  return (
    <div className="workspace-home">
      <header className="workspace-home-header">
        <a href="/" className="brand">
          <BrandMark />
          <span>Wakeit</span>
        </a>
        <div className="top-actions">
          <LanguageSwitch />
          <IconButton
            label={tr('退出登录')}
            className="workspace-signout"
            onClick={() => api('/session', 'DELETE').then(() => location.assign('/'))}
          >
            <ActionIcon name="signOut" />
          </IconButton>
        </div>
      </header>
      <main className="workspace-home-body">
        <div className="content-card">
          <div className="card-header">
            <h1>{tr('工作区')}</h1>
            <IconButton
              label={tr('创建工作区')}
              onClick={() => {
                setStay(true)
                setCreate(true)
              }}
            >
              <ActionIcon name="add" />
            </IconButton>
          </div>
          {error && <ErrorToast error={error} />}
          {workspaces.error ? (
            <div className="card-body">
              <ErrorToast error={workspaces.error.message} />
              <Button onClick={() => void workspaces.refetch()}>{tr('重试')}</Button>
            </div>
          ) : workspaces.isPending ? (
            <div className="card-body quiet text-muted">{tr('加载中…')}</div>
          ) : (
            <>
              {workspaces.data.items.map((workspace) => (
                <a className="workspace-home-row" href={`/w/${workspace.id}/`} key={workspace.id}>
                  <Building2 size={16} />
                  <span>{workspace.name}</span>
                  <small>{roleName(workspace.role)}</small>
                  <ArrowUpRight size={14} />
                </a>
              ))}
              {!workspaces.data.items.length && !workspaces.data.demo && (
                <div className="card-body quiet text-muted">{tr('暂无工作区')}</div>
              )}
              {workspaces.data.demo && (
                <a className="workspace-home-row" href={`/w/${workspaces.data.demo.id}/`}>
                  <Building2 size={16} />
                  <span>{workspaces.data.demo.name}</span>
                  <small>{tr('只读')}</small>
                  <ArrowUpRight size={14} />
                </a>
              )}
            </>
          )}
        </div>
        {user.instance_admin && (
          <a className="workspace-instance-link" href="/instance">
            <Users size={14} aria-hidden="true" />
            {tr('账号管理')}
          </a>
        )}
      </main>
      {create && <CreateWorkspace onClose={() => setCreate(false)} />}
    </div>
  )
}

export function WorkspaceManagement({
  user,
  children,
}: {
  user: User
  children: (name: ReactNode, actions: ReactNode) => ReactNode
}) {
  useTranslation()
  const client = useQueryClient()
  const controls = useSearchControls()
  const editable = user.role === 'admin' && !user.workspace?.is_demo
  const [create, setCreate] = useState(false)
  const [leaving, setLeaving] = useState(false)
  const leave = useMutation({
    mutationFn: () => api(`/workspaces/${workspaceId}/leave`, 'POST', {}),
    onSuccess: async () => {
      await client.cancelQueries()
      location.assign('/')
    },
  })
  const [open, setOpen] = useState(false)
  const [name, setName] = useState(user.workspace?.name ?? '')
  useEffect(() => {
    if (!open) setName(user.workspace?.name ?? '')
  }, [user.workspace?.name, open])
  const save = useMutation({
    mutationFn: () => api(`/workspaces/${workspaceId}`, 'PUT', { name: name.trim() }),
    onSuccess: () => {
      setOpen(false)
      client.invalidateQueries({ queryKey: ['me'] })
      client.invalidateQueries({ queryKey: ['workspaces'] })
    },
  })
  return (
    <>
      {children(
        <div className="setting-row workspace-name-row">
          <span>{tr('工作区名称')}</span>
          <div className="workspace-name-value">
            <span className="workspace-current-name">{user.workspace?.name}</span>
            {editable && (
              <IconButton
                label={tr('更改工作区名称')}
                onClick={() => {
                  save.reset()
                  setOpen(true)
                }}
              >
                <ActionIcon name="edit" />
              </IconButton>
            )}
            {!user.workspace?.is_demo && (
              <IconButton
                label={tr('退出工作区')}
                danger
                onClick={async () => {
                  if (await controls.requestLeave()) {
                    leave.reset()
                    setLeaving(true)
                  }
                }}
              >
                <ActionIcon name="signOut" />
              </IconButton>
            )}
          </div>
        </div>,
        <IconButton
          label={tr('创建工作区')}
          onClick={async () => {
            if (await controls.requestLeave()) setCreate(true)
          }}
        >
          <ActionIcon name="add" />
        </IconButton>,
      )}
      {create && <CreateWorkspace onClose={() => setCreate(false)} />}
      <Modal
        open={leaving}
        onOpenChange={(value) => !leave.isPending && setLeaving(value)}
        title={tr('退出工作区')}
        description={tr('退出「{{v0}}」后，你将失去该工作区的访问权限。', {
          v0: user.workspace?.name,
        })}
      >
        <div className="modal-body">
          {leave.error && <ErrorToast error={leave.error.message} />}
        </div>
        <div className="modal-footer">
          <Button disabled={leave.isPending} onClick={() => setLeaving(false)}>
            {tr('取消')}
          </Button>
          <Button variant="solid" disabled={leave.isPending} onClick={() => leave.mutate()}>
            {tr('退出')}
          </Button>
        </div>
      </Modal>
      <Modal
        open={open}
        onOpenChange={(value) => !save.isPending && setOpen(value)}
        title={tr('更改工作区名称')}
      >
        <form
          onSubmit={(event) => {
            event.preventDefault()
            if (!save.isPending) save.mutate()
          }}
        >
          <div className="modal-body">
            <Field label={tr('工作区名称')}>
              <input
                autoFocus
                required
                maxLength={100}
                value={name}
                disabled={save.isPending}
                onChange={(event) => setName(event.target.value)}
              />
            </Field>
            {save.error && <ErrorToast error={save.error.message} />}
          </div>
          <div className="modal-footer">
            <Button type="button" disabled={save.isPending} onClick={() => setOpen(false)}>
              {tr('取消')}
            </Button>
            <Button variant="solid" type="submit" disabled={save.isPending || !name.trim()}>
              {tr('保存')}
            </Button>
          </div>
        </form>
      </Modal>
    </>
  )
}
