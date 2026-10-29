import { ActionIcon } from '../actions/ActionIcon'
import { ToastContext, ErrorToast } from '../components/Toast'
import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { MomentsVisit } from '../moments/MomentsPage'
import { Button, IconButton } from '../components/ui'
import { Layout } from './Layout'
import { WorkspaceHome } from '../workspaces/Workspaces'
import { InvitationPage } from '../workspaces/InvitationPage'
import { InstanceAccounts } from '../settings/InstanceAccounts'
import { DeviceAuthorization } from '../onboarding/DeviceAuthorization'
import { SearchProvider } from '../search/Search'
import { useState, useCallback, useEffect, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { useQuery } from '@tanstack/react-query'
import { Outlet, useRouterState } from '@tanstack/react-router'
import { RefreshCw, AlertCircle } from '../components/icons'
import { api, ApiError, workspaceId } from '../api/client'
import { type Workspace, type User } from '../api/types'
import { client } from './queryClient'
import { UserContext } from './session'
import { Empty } from '../components/Empty'
import { Login } from './Login'

export function App() {
  useTranslation()
  const me = useQuery<User>({
    queryKey: ['me'],
    queryFn: async () => {
      const account = await api<User>('/session')
      if (!workspaceId) return account
      const workspace = await api<Workspace>(`/workspaces/${workspaceId}`)
      return { ...account, workspace, role: workspace.role }
    },
    retry: false,
    refetchInterval: 10000,
  })
  useEffect(() => {
    const expired = () => {
      client.clear()
      location.assign(location.pathname === '/authorize/device' ? location.href : '/')
    }
    window.addEventListener('wakeit:unauthorized', expired)
    return () => window.removeEventListener('wakeit:unauthorized', expired)
  }, [])
  useEffect(() => {
    if (me.data && me.error instanceof ApiError && me.error.status === 401) {
      client.clear()
      location.assign('/')
    }
  }, [me.error, me.data])
  const [toast, setToast] = useState<{ id: number; content: ReactNode } | null>(null)
  const notify = useCallback((content: ReactNode) => setToast({ id: Date.now(), content }), [])
  const [dark, setDark] = useState(
    () =>
      localStorage.getItem('wakeit-theme') === 'dark' ||
      (!localStorage.getItem('wakeit-theme') && matchMedia('(prefers-color-scheme: dark)').matches),
  )
  useEffect(() => {
    document.documentElement.dataset.theme = dark ? 'dark' : 'light'
    localStorage.setItem('wakeit-theme', dark ? 'dark' : 'light')
  }, [dark])
  useEffect(() => {
    if (toast) {
      const timer = setTimeout(() => setToast(null), 5000)
      return () => clearTimeout(timer)
    }
  }, [toast])
  const path = useRouterState({ select: (s) => s.location.pathname })
  return (
    <ToastContext.Provider value={notify}>
      {path === '/invite' ? (
        <InvitationPage user={me.data} />
      ) : me.isPending ? (
        <Empty title={tr('连接 Wakeit…')} />
      ) : me.error ? (
        me.error instanceof ApiError && me.error.status === 401 ? (
          <Login />
        ) : me.error instanceof ApiError && [403, 404].includes(me.error.status) && workspaceId ? (
          <WorkspaceHome
            user={me.data ?? { id: '', username: '', role: 'viewer' }}
            error={tr('无法访问此工作区')}
          />
        ) : (
          <>
            <ErrorToast error={me.error} />
            <Empty title={tr('无法连接 Gateway')}>
              <Button onClick={() => me.refetch()}>
                <RefreshCw size={14} />
                {tr('重试')}
              </Button>
            </Empty>
          </>
        )
      ) : (
        <UserContext.Provider value={me.data!}>
          <SearchProvider user={me.data!}>
            {path === '/authorize/device' ? (
              <DeviceAuthorization user={me.data!} />
            ) : !workspaceId ? (
              path === '/instance' && me.data?.instance_admin ? (
                <InstanceAccounts user={me.data!} />
              ) : (
                <WorkspaceHome user={me.data!} />
              )
            ) : (
              <Layout
                user={me.data!}
                dark={dark}
                onTheme={() => setDark((v) => !v)}
                onLogout={() =>
                  api('/session', 'DELETE')
                    .then(() => {
                      client.clear()
                      location.assign('/')
                    })
                    .catch((e) => notify(e.message))
                }
              >
                <MomentsVisit>
                  <Outlet />
                </MomentsVisit>
              </Layout>
            )}
          </SearchProvider>
        </UserContext.Provider>
      )}
      {createPortal(
        <div className="toast-viewport" aria-live="assertive" aria-atomic="true">
          {toast && (
            <div className="toast" role="alert" key={toast.id}>
              <AlertCircle size={15} />
              <div className="toast-content">{toast.content}</div>
              <IconButton label={tr('关闭提示')} onClick={() => setToast(null)}>
                <ActionIcon name="close" />
              </IconButton>
            </div>
          )}
        </div>,
        document.body,
      )}
    </ToastContext.Provider>
  )
}
