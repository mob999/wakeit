import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import * as Dropdown from '@radix-ui/react-dropdown-menu'
import { Building2, Check, ChevronDown, Plus, LogOut, ArrowUpRight, FileCog } from 'lucide-react'
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
          {create.error && (
            <p className="error" role="alert">
              {create.error.message}
            </p>
          )}
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
  const [create, setCreate] = useState(false)
  const [leaving, setLeaving] = useState(false)
  const leave = useMutation({
    mutationFn: () => api(`/workspaces/${workspaceId}/leave`, 'POST', {}),
    onSuccess: () => location.assign('/'),
  })
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
    <>
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
          <Dropdown.Content className="dropdown workspace-menu" align="end">
            <Dropdown.Label className="user-menu-label">
              {user.username}
              <small>
                {user.workspace?.name} — {roleName(user.role)}
              </small>
            </Dropdown.Label>
            <Dropdown.Label className="workspace-menu-label">{tr('工作区')}</Dropdown.Label>
            {workspaces.isPending && <Dropdown.Item disabled>{tr('加载中…')}</Dropdown.Item>}
            {workspaces.error && (
              <Dropdown.Item onSelect={() => void workspaces.refetch()}>{tr('重试')}</Dropdown.Item>
            )}
            {[
              ...(workspaces.data?.items ?? []),
              ...(workspaces.data?.demo ? [workspaces.data.demo] : []),
            ].map((workspace) => (
              <Dropdown.Item key={workspace.id} onSelect={() => void select(workspace.id)}>
                <Building2 size={14} />
                <span>{workspace.name}</span>
                {workspace.id === workspaceId && <Check size={14} className="workspace-check" />}
              </Dropdown.Item>
            ))}
            <Dropdown.Item
              onSelect={() => controls.requestLeave().then((allowed) => allowed && setCreate(true))}
            >
              <Plus size={14} />
              {tr('创建工作区')}
            </Dropdown.Item>
            {!user.workspace?.is_demo && (
              <Dropdown.Item
                onSelect={() =>
                  controls.requestLeave().then((allowed) => {
                    if (allowed) {
                      leave.reset()
                      setLeaving(true)
                    }
                  })
                }
              >
                <LogOut size={14} />
                {tr('退出工作区')}
              </Dropdown.Item>
            )}
            {user.instance_admin && (
              <Dropdown.Item
                onSelect={() =>
                  controls.requestLeave().then((allowed) => allowed && location.assign('/instance'))
                }
              >
                {tr('实例账号管理')}
              </Dropdown.Item>
            )}
            <Dropdown.Item
              onSelect={() => controls.requestLeave().then((allowed) => allowed && onLogout())}
            >
              <LogOut size={14} />
              {tr('退出登录')}
            </Dropdown.Item>
          </Dropdown.Content>
        </Dropdown.Portal>
      </Dropdown.Root>
      {create && <CreateWorkspace onClose={() => setCreate(false)} />}
      <Modal
        open={leaving}
        onOpenChange={(open) => !leave.isPending && setLeaving(open)}
        title={tr('退出工作区')}
        description={tr('退出「{{v0}}」后，你将失去该工作区的访问权限。', {
          v0: user.workspace?.name,
        })}
      >
        <div className="modal-body">
          {leave.error && (
            <p className="error" role="alert">
              {leave.error.message}
            </p>
          )}
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
    </>
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
            onClick={() => api('/session', 'DELETE').then(() => location.assign('/'))}
          >
            <LogOut />
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
              <Plus />
            </IconButton>
          </div>
          {error && (
            <p className="error card-body" role="alert">
              {error}
            </p>
          )}
          {workspaces.error ? (
            <div className="card-body">
              <p className="error">{workspaces.error.message}</p>
              <Button onClick={() => void workspaces.refetch()}>{tr('重试')}</Button>
            </div>
          ) : workspaces.isPending ? (
            <div className="card-body quiet">{tr('加载中…')}</div>
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
                <div className="card-body quiet">{tr('暂无工作区')}</div>
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
            {tr('实例账号管理')}
          </a>
        )}
      </main>
      {create && <CreateWorkspace onClose={() => setCreate(false)} />}
    </div>
  )
}

export function WorkspaceName({ user }: { user: User }) {
  useTranslation()
  const client = useQueryClient()
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
      <div className="content-card workspace-name-settings">
        <div className="setting-row">
          <span>{tr('工作区名称')}</span>
          <span className="workspace-current-name">{user.workspace?.name}</span>
          <IconButton
            label={tr('更改工作区名称')}
            onClick={() => {
              save.reset()
              setOpen(true)
            }}
          >
            <FileCog />
          </IconButton>
        </div>
      </div>
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
            {save.error && (
              <p className="error" role="alert">
                {save.error.message}
              </p>
            )}
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
