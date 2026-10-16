import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useEffect, useId, useRef, useState } from 'react'
import { useInfiniteQuery, useQuery } from '@tanstack/react-query'
import { ArrowLeft, LoaderCircle, RefreshCw } from 'lucide-react'
import { api, type Agent, type Runner, type SessionCatalog } from '../api'
import { ChoicePicker } from '../ChoicePicker'
import { Button, IconButton } from '../ui'

export function SessionPicker({
  runner,
  agent,
  cwd,
  id,
  disabled,
  onChange,
}: {
  runner?: Runner
  agent?: Agent
  cwd: string
  id: string
  disabled: boolean
  onChange: (id: string) => void
}) {
  useTranslation()
  const controlId = useId()
  const [open, setOpen] = useState(false)
  const [manual, setManual] = useState(false)
  const [draft, setDraft] = useState('')
  const [search, setSearch] = useState('')
  const [query, setQuery] = useState('')
  const [labels, setLabels] = useState(new Map<string, string>())
  const inputRef = useRef<HTMLInputElement>(null)
  const canQuery = !!runner?.online && !!agent?.session_list_supported && !!cwd

  useEffect(() => {
    const timer = setTimeout(() => setQuery(search), 200)
    return () => clearTimeout(timer)
  }, [search])
  useEffect(() => {
    if (open && manual) inputRef.current?.focus()
  }, [open, manual])
  const canInspect = !!id && !!runner?.online && !!agent && !!cwd
  const inspection = useQuery({
    queryKey: ['session-inspection', runner?.id, agent?.id, agent?.digest, cwd, id],
    queryFn: () =>
      api<{ title?: string | null }>(
        `/runners/${runner!.id}/agents/${encodeURIComponent(agent!.id)}/sessions/inspect`,
        'POST',
        { cwd, id },
      ),
    enabled: canInspect,
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    gcTime: 0,
  })
  const checking = canInspect && inspection.isFetching
  const error = canInspect && inspection.isError ? inspection.error.message : ''
  useEffect(() => {
    if (inspection.data && 'title' in inspection.data)
      setLabels((previous) => new Map(previous).set(id, inspection.data.title?.trim() || ''))
  }, [inspection.data, id])

  const catalog = useInfiniteQuery({
    queryKey: ['sessions', runner?.id, agent?.id, agent?.digest, cwd, query],
    queryFn: ({ pageParam }) =>
      api<SessionCatalog>(
        `/runners/${runner!.id}/agents/${encodeURIComponent(agent!.id)}/sessions/query`,
        'POST',
        { cwd, ...(query ? { q: query } : {}), ...(pageParam ? { cursor: pageParam } : {}) },
      ),
    initialPageParam: null as string | null,
    getNextPageParam: (last) => last.next_cursor || undefined,
    enabled: open && !manual && canQuery && !disabled,
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    gcTime: 0,
  })
  const sessions = Array.from(
    new Map(
      (catalog.data?.pages.flatMap((page) => page.sessions) ?? []).map((session) => [
        session.id,
        session,
      ]),
    ).values(),
  )
  function choose(next: string) {
    const selected = sessions.find((session) => session.id === next)
    if (selected) setLabels((previous) => new Map(previous).set(next, selected.title?.trim() || ''))
    if (next === id && canInspect) void inspection.refetch()
    onChange(next)
  }
  function changeOpen(next: boolean) {
    setOpen(next)
    setManual(false)
  }
  const unavailable = !cwd
    ? tr('先选择工作目录')
    : !runner?.online
      ? tr('设备离线')
      : !agent?.session_list_supported
        ? ''
        : undefined
  return (
    <div className="field session-binding">
      <label htmlFor={controlId}>{tr('会话')}</label>
      <ChoicePicker
        id={controlId}
        label={tr('会话')}
        value={id}
        open={open}
        searchable={canQuery}
        filterOptions={false}
        selectionLabel={
          id
            ? labels.has(id)
              ? labels.get(id) || tr('未命名会话')
              : sessions.find((session) => session.id === id)?.title?.trim() || id
            : tr('每次新建')
        }
        disabled={disabled}
        indicator={
          checking ? (
            <LoaderCircle size={14} className="spin" aria-label={tr('正在检查会话')} />
          ) : undefined
        }
        aria-describedby={error ? `${controlId}-error` : undefined}
        pending={
          canQuery &&
          (search !== query ||
            catalog.isPending ||
            (catalog.isFetching && !catalog.isFetchingNextPage))
        }
        error={canQuery && catalog.isError ? catalog.error.message : undefined}
        emptyLabel={unavailable ?? tr('未找到会话')}
        onOpenChange={changeOpen}
        onSearchChange={setSearch}
        pinnedOptions={[{ value: '', label: tr('每次新建') }]}
        options={
          canQuery
            ? sessions.map((session) => ({
                value: session.id,
                label: session.title?.trim() || tr('未命名会话'),
                description: session.id,
                wrapDescription: true,
              }))
            : []
        }
        onChange={choose}
        content={
          manual ? (
            <div className="session-manual">
              <div className="session-manual-heading">
                <IconButton
                  label={tr('返回会话列表')}
                  disabled={disabled}
                  onClick={() => setManual(false)}
                >
                  <ArrowLeft />
                </IconButton>
                <label htmlFor={`${controlId}-manual`}>{tr('会话 ID')}</label>
              </div>
              <input
                ref={inputRef}
                id={`${controlId}-manual`}
                className="mono"
                value={draft}
                autoComplete="off"
                disabled={disabled}
                onChange={(event) => setDraft(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter' && !event.nativeEvent.isComposing) {
                    event.preventDefault()
                    event.stopPropagation()
                    if (!disabled && draft.trim()) {
                      choose(draft.trim())
                      changeOpen(false)
                    }
                  }
                }}
              />
              <div className="session-manual-actions">
                <Button
                  disabled={disabled || !draft.trim()}
                  onClick={() => {
                    choose(draft.trim())
                    changeOpen(false)
                  }}
                >
                  {tr('使用')}
                </Button>
              </div>
            </div>
          ) : undefined
        }
        footer={
          <>
            <Button
              className="session-manual-entry"
              disabled={disabled}
              onClick={() => {
                setDraft(id)
                setManual(true)
              }}
            >
              {tr('输入会话 ID…')}
            </Button>
            {canQuery && (
              <IconButton
                label={tr('刷新会话')}
                disabled={disabled || catalog.isFetching}
                loading={catalog.isRefetching}
                onClick={() => catalog.refetch()}
              >
                <RefreshCw />
              </IconButton>
            )}
            {canQuery && catalog.hasNextPage && !catalog.isError && (
              <Button
                disabled={disabled || catalog.isFetching}
                loading={catalog.isFetchingNextPage}
                onClick={() => catalog.fetchNextPage()}
              >
                {tr('加载更多')}
              </Button>
            )}
          </>
        }
      />
      {error && (
        <p className="error" id={`${controlId}-error`} role="alert">
          {error}
        </p>
      )}
    </div>
  )
}
