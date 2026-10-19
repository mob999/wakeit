import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useCallback, useEffect, useRef, useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useBlocker, useNavigate, useSearch } from '@tanstack/react-router'
import { api, type User, type WorkspaceSettings } from '../api'
import { matches, settingSections, useSearchControls } from '../Search'
import { Button, Modal } from '../ui'
import { Split, DetailBody } from '../Workbench'
import { Globe, Users, KeyRound, Archive } from 'lucide-react'
import { RequestState } from './shared'
import { WorkspaceSettingsView, draftErrors, type WorkspaceDraft } from './WorkspaceSettings'
import { MembersSettings } from './MembersSettings'
import { WorkspaceManagement } from '../Workspaces'
import { ArchivedTasks } from './ArchivedTasks'
import { TokenSettings } from './TokenSettings'
const toDraft = (s: WorkspaceSettings): WorkspaceDraft => ({
  timezone: s.timezone,
  log_retention_days: String(s.log_retention_days),
  run_retention_days: String(s.run_retention_days),
})
export function SettingsPage({ user, toast }: { user: User; toast: (message: string) => void }) {
  useTranslation()
  const navigate = useNavigate()
  const route = useSearch({ from: '/settings' })
  const controls = useSearchControls()
  const client = useQueryClient()
  const admin = user.role === 'admin' && !user.workspace?.is_demo
  const sections = settingSections(user.role, !!user.workspace?.is_demo)
  const section = sections.some((s) => s.id === route.section) ? route.section! : sections[0].id
  const q = controls.filters.settings?.q ?? ''
  const noMatches = !!q && !sections.some((s) => matches(`${s.name} ${s.terms}`, q))
  const settings = useQuery<WorkspaceSettings>({
    queryKey: ['settings'],
    queryFn: () => api('/settings'),
    enabled: section === 'workspace',
  })
  const [baseline, setBaseline] = useState<WorkspaceSettings | null>(null)
  const [draft, setDraft] = useState<WorkspaceDraft | null>(null)
  const [saveError, setSaveError] = useState('')
  const dirty =
    admin && !!baseline && !!draft && JSON.stringify(draft) !== JSON.stringify(toDraft(baseline))
  const dirtyRef = useRef(dirty)
  dirtyRef.current = dirty
  const [leaveOpen, setLeaveOpen] = useState(false)
  const leavePromise = useRef<Promise<boolean> | null>(null)
  const leaveResolve = useRef<((allowed: boolean) => void) | null>(null)
  const requestLeave = useCallback(() => {
    if (!dirtyRef.current) return Promise.resolve(true)
    if (leavePromise.current) return leavePromise.current
    setLeaveOpen(true)
    leavePromise.current = new Promise((resolve) => {
      leaveResolve.current = resolve
    })
    return leavePromise.current
  }, [])
  const finishLeave = (allowed: boolean) => {
    leaveResolve.current?.(allowed)
    leaveResolve.current = null
    leavePromise.current = null
    setLeaveOpen(false)
  }
  useEffect(() => controls.registerGuard(requestLeave), [controls.registerGuard, requestLeave])
  useEffect(
    () => () => {
      leaveResolve.current?.(false)
    },
    [],
  )
  useBlocker({
    shouldBlockFn: ({ current, next }) =>
      current.fullPath === '/settings' &&
      next.fullPath === '/settings' &&
      current.search.section === next.search.section
        ? false
        : requestLeave().then((allowed) => !allowed),
    enableBeforeUnload: () => dirtyRef.current,
    disabled: !admin,
  })
  useEffect(() => {
    if (settings.data) {
      setBaseline(settings.data)
      if (!dirtyRef.current || !admin) setDraft(toDraft(settings.data))
    }
    if (!admin) setSaveError('')
  }, [settings.data, admin])
  useEffect(() => {
    if (route.section !== undefined && route.section !== section)
      navigate({ to: '/settings', search: { section }, replace: true })
  }, [section, route.section, navigate])
  const handledQ = useRef<string | undefined>(undefined)
  useEffect(() => {
    if (handledQ.current === q) return
    handledQ.current = q
    if (!q) return
    const match =
      sections.find((s) => s.id === section && matches(`${s.name} ${s.terms}`, q)) ??
      sections.find((s) => matches(`${s.name} ${s.terms}`, q))
    if (match && match.id !== section)
      navigate({ to: '/settings', search: { section: match.id }, replace: true })
  }, [q, section, user.role, navigate])
  const saveMutation = useMutation({
    mutationFn: (value: WorkspaceSettings) => api<WorkspaceSettings>('/settings', 'PUT', value),
    onSuccess: (value) => {
      dirtyRef.current = false
      setBaseline(value)
      setDraft(toDraft(value))
      setSaveError('')
      client.setQueryData(['settings'], value)
      toast(tr('已保存'))
    },
    onError: (e: Error) => setSaveError(e.message),
  })
  const save = async () => {
    if (!admin || !draft || Object.keys(draftErrors(draft)).length) return false
    setSaveError('')
    try {
      await saveMutation.mutateAsync({
        timezone: draft.timezone,
        log_retention_days: Number(draft.log_retention_days),
        run_retention_days: Number(draft.run_retention_days),
      })
      return true
    } catch {
      return false
    }
  }
  const cancel = () => {
    if (baseline) setDraft(toDraft(baseline))
    setSaveError('')
    dirtyRef.current = false
  }
  const clearFilter = () => controls.setFilters({ ...controls.filters, settings: {} })
  const changeSection = async (id: string) => {
    if (id === section && !q && route.section === id) return
    if (await requestLeave()) {
      clearFilter()
      navigate({ to: '/settings', search: { section: id } })
    }
  }
  return (
    <>
      <Split
        label={tr('Settings')}
        crumbs={[
          {
            label: section === 'tokens' ? 'API Key' : sections.find((s) => s.id === section)!.name,
          },
        ]}
        detailOpen={!!route.section}
        onBack={async () => {
          if (await requestLeave()) navigate({ to: '/settings', search: {} })
        }}
        list={
          <>
            <div className="pane-heading">
              <h1>{tr('Settings')}</h1>
            </div>
            <div className="list-scroll" role="navigation" aria-label={tr('设置分类')}>
              {sections.map((s) => {
                const Icon =
                  s.id === 'workspace'
                    ? Globe
                    : s.id === 'members'
                      ? Users
                      : s.id === 'archived'
                        ? Archive
                        : KeyRound
                return (
                  <button
                    key={s.id}
                    className={`task-row settings-category ${section === s.id ? 'active' : ''}`}
                    aria-current={section === s.id ? 'page' : undefined}
                    onClick={() => changeSection(s.id)}
                  >
                    <Icon size={16} />
                    <span>{s.id === 'tokens' ? 'API Key' : s.name}</span>
                  </button>
                )
              })}
            </div>
          </>
        }
      >
        {noMatches ? (
          <>
            <DetailBody>
              <div className="settings-request">
                <span>{tr('无匹配设置')}</span>
                <Button onClick={clearFilter}>{tr('清除筛选')}</Button>
              </div>
            </DetailBody>
          </>
        ) : section === 'workspace' ? (
          <>
            <DetailBody limited>
              {!draft ? (
                <RequestState
                  pending={settings.isPending}
                  error={settings.error}
                  retry={() => settings.refetch()}
                />
              ) : (
                <>
                  {settings.error && (
                    <RequestState
                      pending={false}
                      error={settings.error}
                      retry={() => settings.refetch()}
                    />
                  )}
                  <WorkspaceManagement user={user}>
                    {(name, actions) => (
                      <WorkspaceSettingsView
                        workspaceName={name}
                        workspaceActions={actions}
                        readOnly={!admin}
                        draft={draft}
                        dirty={dirty}
                        pending={saveMutation.isPending}
                        error={saveError}
                        onChange={setDraft}
                        onCancel={cancel}
                        onSave={() => save()}
                      />
                    )}
                  </WorkspaceManagement>
                </>
              )}
            </DetailBody>
          </>
        ) : section === 'members' ? (
          <MembersSettings user={user} />
        ) : section === 'archived' ? (
          <ArchivedTasks />
        ) : (
          <TokenSettings />
        )}
      </Split>
      <Modal
        open={leaveOpen}
        onOpenChange={(v) => {
          if (!v && !saveMutation.isPending) finishLeave(false)
        }}
        title={tr('有未保存的更改')}
      >
        <div className="modal-body">
          {saveError && (
            <p className="error" role="alert">
              {saveError}
            </p>
          )}
        </div>
        <div className="modal-footer">
          <Button disabled={saveMutation.isPending} onClick={() => finishLeave(false)}>
            {tr('继续编辑')}
          </Button>
          <Button
            disabled={saveMutation.isPending}
            onClick={() => {
              cancel()
              finishLeave(true)
            }}
          >
            {tr('放弃更改')}
          </Button>
          <Button
            variant="solid"
            disabled={
              saveMutation.isPending || !draft || Object.keys(draftErrors(draft)).length > 0
            }
            onClick={async () => {
              if (await save()) finishLeave(true)
            }}
          >
            {saveMutation.isPending ? tr('正在保存…') : tr('保存并离开')}
          </Button>
        </div>
      </Modal>
    </>
  )
}
