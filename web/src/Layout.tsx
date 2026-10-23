import { ActionIcon } from './actions/ActionIcon'
import { t as tr } from './i18n'
import { useTranslation } from 'react-i18next'
import { useEffect, useState, type ReactNode } from 'react'
import { Link } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { ListTodo, Rss, History, Monitor, Settings2 } from './icons'
import { api, type User } from './api'
import { IconButton, TooltipTarget } from './ui'
import { SearchLauncher } from './Search'
import { PaneResizeHandle } from './PaneResizeHandle'
import { PageHeaderContext } from './PageHeader'
import { DisplayPreferences } from './actions/DisplayPreferences'
import { BrandMark } from './BrandMark'
import { WorkspaceAccountMenu } from './Workspaces'
import { AgentPrompt, type GatewayMeta } from './onboarding/AgentPrompt'
import { Changelog } from './Changelog'
export function Layout({
  user,
  dark,
  onTheme,
  onLogout,
  children,
}: {
  user: User
  dark: boolean
  onTheme: () => void
  onLogout: () => void
  children: ReactNode
}) {
  useTranslation()
  const [breadcrumbs, setBreadcrumbs] = useState<HTMLDivElement | null>(null)
  const [actions, setActions] = useState<HTMLDivElement | null>(null)
  const meta = useQuery<GatewayMeta>({
    queryKey: ['meta'],
    queryFn: () => api('/meta'),
    staleTime: Infinity,
  })
  const [preferredWidth, setWidth] = useState(() =>
    Math.max(128, Math.min(280, Number(localStorage.getItem('wakeit-navigation-width')) || 144)),
  )
  const [desktopCollapsed, setCollapsed] = useState(
    () => localStorage.getItem('wakeit-navigation-collapsed') === 'true',
  )
  const [viewport, setViewport] = useState(window.innerWidth)
  const [mobileOpen, setMobileOpen] = useState(false)
  useEffect(() => {
    const update = () => {
      setViewport(window.innerWidth)
      setMobileOpen(false)
    }
    window.addEventListener('resize', update)
    return () => window.removeEventListener('resize', update)
  }, [])
  const narrow = viewport <= 720
  const collapsed = narrow ? !mobileOpen : desktopCollapsed
  const max = Math.min(280, Math.max(128, viewport - 360 - (viewport > 1000 ? 270 : 0)))
  const width = Math.min(preferredWidth, max)
  const resize = (next: number) => {
    const value = Math.max(128, Math.min(max, next))
    setWidth(value)
    localStorage.setItem('wakeit-navigation-width', String(value))
  }
  const toggle = () => {
    if (narrow) setMobileOpen((v) => !v)
    else {
      setCollapsed((v) => !v)
      localStorage.setItem('wakeit-navigation-collapsed', String(!desktopCollapsed))
    }
  }
  return (
    <PageHeaderContext.Provider value={{ breadcrumbs, actions }}>
      <div className="app">
        <header className="topbar">
          <div
            className={`header-navigation ${collapsed ? 'header-navigation-collapsed' : ''}`}
            style={{ width: collapsed ? 44 : width }}
          >
            {collapsed ? (
              <IconButton
                label={tr('展开导航')}
                className="brand-expand"
                aria-controls="primary-navigation"
                aria-expanded={false}
                onClick={toggle}
              >
                <BrandMark />
              </IconButton>
            ) : (
              <>
                <Link to="/" className="brand">
                  <BrandMark />
                  <span>Wakeit</span>
                </Link>
                <IconButton
                  label={tr('折叠导航')}
                  aria-controls="primary-navigation"
                  aria-expanded={true}
                  onClick={toggle}
                >
                  <ActionIcon name="sidebar" />
                </IconButton>
              </>
            )}
          </div>
          <div className="header-main">
            <div className="header-breadcrumbs" ref={setBreadcrumbs} />
            <div className="header-page-actions" ref={setActions} />
            <div className="top-actions">
              <Changelog version={meta.data?.version} />
              <AgentPrompt meta={meta.data} workspace={user.workspace} />
              <DisplayPreferences dark={dark} onTheme={onTheme} />
              <WorkspaceAccountMenu user={user} onLogout={onLogout} />
            </div>
          </div>
        </header>
        <div className="workbench">
          {narrow && mobileOpen && (
            <button
              className="navigation-backdrop"
              aria-label={tr('关闭导航')}
              onClick={() => setMobileOpen(false)}
            />
          )}
          <div className="navigation-frame" style={{ width: narrow || collapsed ? 44 : width }}>
            <aside
              id="primary-navigation"
              aria-label={tr('主导航')}
              className={`navigation ${collapsed ? 'collapsed' : ''} ${narrow && mobileOpen ? 'navigation-overlay' : ''}`}
              style={narrow && mobileOpen ? { width } : undefined}
            >
              <SearchLauncher collapsed={collapsed} onOpen={() => setMobileOpen(false)} />
              {[
                { to: '/', icon: Rss, label: tr('Moments') },
                { to: '/tasks', icon: ListTodo, label: tr('Tasks') },
                { to: '/runs', icon: History, label: tr('Runs') },
                { to: '/runners', icon: Monitor, label: tr('Runners') },
                { to: '/settings', icon: Settings2, label: tr('Settings') },
              ].map(({ to, icon: Icon, label }) => (
                <TooltipTarget key={to} label={label} enabled={collapsed}>
                  <Link
                    to={to}
                    aria-label={label}
                    activeOptions={{ exact: to === '/' }}
                    className="nav-item"
                    activeProps={{ className: 'nav-item selected' }}
                    onClick={() => setMobileOpen(false)}
                  >
                    <Icon size={16} aria-hidden="true" />
                    <span className="nav-label">{label}</span>
                  </Link>
                </TooltipTarget>
              ))}
              <TooltipTarget label="@mob999/wakeit" enabled={collapsed}>
                <a
                  className="nav-bottom repository-link"
                  href="https://github.com/mob999/wakeit"
                  target="_blank"
                  rel="noopener noreferrer"
                  aria-label="GitHub: @mob999/wakeit"
                >
                  <img
                    className="repository-mark"
                    src={dark ? '/github-mark-white.svg' : '/github-mark-black.svg'}
                    width={16}
                    height={16}
                    alt=""
                  />
                  <span className="nav-label">@mob999/wakeit</span>
                </a>
              </TooltipTarget>
            </aside>
            {!collapsed && !narrow && (
              <PaneResizeHandle
                label={tr('调整导航宽度')}
                width={width}
                min={128}
                max={max}
                onResize={resize}
                onReset={() => resize(144)}
                style={{ right: -4, top: -44 }}
              />
            )}
          </div>
          <main className="main">{children}</main>
        </div>
      </div>
    </PageHeaderContext.Provider>
  )
}
