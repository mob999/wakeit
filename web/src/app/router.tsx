import { PresetPage } from '../templates/PresetPage'
import { useToast } from '../components/Toast'
import { useTranslation } from 'react-i18next'
import { MomentsPage } from '../moments/MomentsPage'
import { SettingsPage } from '../settings/SettingsPage'
import { RunnersPage } from '../runners/RunnersPage'
import { createRootRoute, createRoute, createRouter } from '@tanstack/react-router'
import { workspaceBase } from '../api/client'
import { App } from './App'
import { useUser } from './session'
import { TasksPage } from '../tasks/TasksPage'
import { RunsPage } from '../runs/RunsPage'
import { DagScreen } from '../dag/DagScreen'

function SettingsScreen() {
  useTranslation()
  return <SettingsPage user={useUser()} toast={useToast()} />
}

const rootRoute = createRootRoute({ component: App })

const momentsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/',
  component: () => <MomentsPage toast={useToast()} />,
})

const tasksRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/tasks',
  validateSearch: (s: Record<string, unknown>): { task?: string } => ({
    task: typeof s.task === 'string' ? s.task : undefined,
  }),
  component: () => <TasksPage user={useUser()} search={tasksRoute.useSearch()} />,
})

const presetsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/presets',
  validateSearch: (s: Record<string, unknown>): { template?: string } => ({
    template: typeof s.template === 'string' ? s.template : undefined,
  }),
  component: () => <PresetPage user={useUser()} />,
})

const dagRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/tasks/dag',
  validateSearch: (s: Record<string, unknown>): { task?: string } => ({
    task: typeof s.task === 'string' ? s.task : undefined,
  }),
  component: () => <DagScreen user={useUser()} search={dagRoute.useSearch()} />,
})

const runsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/runs',
  validateSearch: (search: Record<string, unknown>): { run?: string } => ({
    run: typeof search.run === 'string' ? search.run : undefined,
  }),
  component: () => <RunsPage user={useUser()} search={runsRoute.useSearch()} />,
})

const runnersRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/runners',
  validateSearch: (
    s: Record<string, unknown>,
  ): { runner?: string; section?: 'overview' | 'agents'; agent?: string } => ({
    runner: typeof s.runner === 'string' ? s.runner : undefined,
    section: s.section === 'agents' || s.section === 'overview' ? s.section : undefined,
    agent: typeof s.agent === 'string' ? s.agent : undefined,
  }),
  component: () => <RunnersPage user={useUser()} toast={useToast()} />,
})

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/settings',
  validateSearch: (s: Record<string, unknown>): { section?: string } => ({
    section: typeof s.section === 'string' ? s.section : undefined,
  }),
  component: SettingsScreen,
})

const inviteRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/invite',
  component: () => null,
})

const authorizationRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/authorize/device',
  component: () => null,
})

const instanceRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/instance',
  component: () => null,
})

const workspaceHomeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/workspaces',
  component: () => null,
})

const demoRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: '/demo',
  component: () => null,
})

export const router = createRouter({
  basepath: workspaceBase || '/',
  routeTree: rootRoute.addChildren([
    momentsRoute,
    tasksRoute,
    presetsRoute,
    dagRoute,
    runsRoute,
    runnersRoute,
    settingsRoute,
    inviteRoute,
    authorizationRoute,
    instanceRoute,
    workspaceHomeRoute,
    demoRoute,
  ]),
})

declare module '@tanstack/react-router' {
  interface Register {
    router: typeof router
  }
}
