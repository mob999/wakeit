import { useState, useRef, useEffect, useId, type KeyboardEvent } from 'react'
import { useInfiniteQuery, useQueryClient } from '@tanstack/react-query'
import { Folder, FolderRoot, ArrowUp, RefreshCw, Eye, EyeOff, ChevronRight } from 'lucide-react'
import { api, type Runner, type DirectoryListing } from '../api'
import { Button, IconButton } from '../ui'
import { directoryInput, directoryPath } from './paths'
import './directories.css'

export function DirectoryBrowser({
  runner,
  initial,
  onSelect,
  onCancel,
}: {
  runner: Runner
  initial: string | null
  onSelect: (path: string) => void
  onCancel: () => void
}) {
  const client = useQueryClient()
  const listId = useId()
  const input = useRef<HTMLInputElement>(null)
  const queued = useRef<'open' | 'select' | null>(null)
  const alive = useRef(true)
  useEffect(() => {
    alive.current = true
    return () => {
      alive.current = false
    }
  }, [])
  useEffect(() => {
    const frame = requestAnimationFrame(() => input.current?.focus())
    return () => cancelAnimationFrame(frame)
  }, [])
  const [base, setBase] = useState(initial)
  const [draft, setDraft] = useState(directoryPath(initial))
  const [query, setQuery] = useState(draft)
  const [active, setActive] = useState(-1)
  const [hidden, setHidden] = useState(false)
  const [error, setError] = useState('')
  const [selecting, setSelecting] = useState(false)
  const location = directoryInput(query, base)
  useEffect(() => {
    const timer = setTimeout(() => setQuery(draft), 180)
    return () => clearTimeout(timer)
  }, [draft])
  const listing = useInfiniteQuery({
    queryKey: ['directories', runner.id, location.path],
    queryFn: ({ pageParam }) =>
      api<DirectoryListing>(
        `/runners/${runner.id}/directories?${new URLSearchParams({ ...(location.path ? { path: location.path } : {}), ...(pageParam ? { cursor: pageParam } : {}) })}`,
      ),
    initialPageParam: null as string | null,
    getNextPageParam: (last) => last.next_cursor,
    enabled: runner.online,
    retry: false,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    staleTime: 0,
  })
  const data = listing.data?.pages[0]
  const directories = listing.data?.pages.flatMap((page) => page.directories) ?? []
  const needle = location.filter.toLocaleLowerCase()
  const entries = directories.filter(
    (d) =>
      (hidden || !d.name.startsWith('.') || needle.startsWith('.')) &&
      d.name.toLocaleLowerCase().includes(needle),
  )
  const pending = draft !== query
  const busy = listing.isFetching || pending || selecting
  // Search the complete current directory, including items after the first 200.
  useEffect(() => {
    if (needle && listing.hasNextPage && !listing.isFetching && !listing.error)
      listing.fetchNextPage()
  }, [needle, listing.hasNextPage, listing.isFetching, listing.error, listing.fetchNextPage])
  useEffect(() => {
    document.getElementById(`${listId}-${active}`)?.scrollIntoView({ block: 'nearest' })
  }, [active, listId])
  const selected = active >= 0 ? entries[active]?.path : !needle ? data?.path : undefined
  function browse(next: string | null) {
    queued.current = null
    setBase(next)
    setDraft(directoryPath(next))
    setQuery(directoryPath(next))
    setActive(-1)
    setError('')
    input.current?.focus()
  }
  async function choose() {
    if (!selected || busy || listing.error || !runner.online) return
    setSelecting(true)
    setError('')
    try {
      // Revalidate the selection: it may have been removed or changed into an escaping symlink.
      const checked = await client.fetchQuery({
        queryKey: ['directory-selection', runner.id, selected],
        queryFn: () =>
          api<DirectoryListing>(
            `/runners/${runner.id}/directories?${new URLSearchParams({ path: selected })}`,
          ),
        staleTime: 0,
        retry: false,
      })
      if (!checked.path) throw new Error('请选择目录')
      if (alive.current) onSelect(checked.path)
    } catch (e) {
      if (alive.current) {
        setError((e as Error).message)
        setSelecting(false)
      }
    }
  }
  function accept(action: 'open' | 'select') {
    if (busy) {
      queued.current = action
      return
    }
    if (listing.error) return
    if (action === 'select') {
      void choose()
      return
    }
    if (entries[active]) browse(entries[active].path)
    else if (needle) browse((location.path ?? '') + location.filter)
    else void choose()
  }
  useEffect(() => {
    if (listing.error) {
      queued.current = null
      return
    }
    if (!busy && queued.current) {
      const action = queued.current
      queued.current = null
      accept(action)
    }
  }, [busy, listing.error])
  function key(event: KeyboardEvent<HTMLInputElement>) {
    if (event.nativeEvent.isComposing || selecting) return
    if (event.altKey && event.key === 'ArrowUp' && data?.parent) {
      event.preventDefault()
      browse(data.parent)
      return
    }
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      setActive((i) =>
        entries.length
          ? i < 0
            ? event.key === 'ArrowDown'
              ? 0
              : entries.length - 1
            : (i + (event.key === 'ArrowDown' ? 1 : -1) + entries.length) % entries.length
          : -1,
      )
      return
    }
    if (
      (event.key === 'Tab' || event.key === 'ArrowRight') &&
      entries[active] &&
      (event.key === 'Tab' || input.current?.selectionStart === draft.length)
    ) {
      event.preventDefault()
      accept('open')
      return
    }
    if (event.key === 'Enter') {
      event.preventDefault()
      accept(event.metaKey || event.ctrlKey ? 'select' : 'open')
    }
  }

  return (
    <>
      <div className="modal-body directory-quick-picker">
        <div className="directory-quick-toolbar">
          <IconButton label="允许的根目录" disabled={selecting} onClick={() => browse(null)}>
            <FolderRoot />
          </IconButton>
          <IconButton
            label="上一级"
            disabled={!data?.parent || selecting || listing.isFetching}
            onClick={() => browse(data?.parent ?? null)}
          >
            <ArrowUp />
          </IconButton>
          <input
            ref={input}
            className="mono directory-input"
            aria-label="目录路径"
            role="combobox"
            aria-autocomplete="list"
            aria-expanded={true}
            aria-controls={listId}
            aria-activedescendant={entries[active] ? `${listId}-${active}` : undefined}
            autoComplete="off"
            spellCheck={false}
            value={draft}
            disabled={selecting}
            placeholder="输入路径或目录名"
            onKeyDown={key}
            onChange={(e) => {
              queued.current = null
              setDraft(e.target.value)
              setActive(0)
              setError('')
            }}
          />
          <IconButton
            label={hidden ? '隐藏点开头的目录' : '显示隐藏目录'}
            aria-pressed={hidden}
            disabled={selecting}
            onClick={() => {
              setHidden((v) => !v)
              setActive(-1)
            }}
          >
            {hidden ? <EyeOff /> : <Eye />}
          </IconButton>
          <IconButton
            label="刷新目录"
            loading={listing.isRefetching}
            disabled={selecting || listing.isFetching}
            onClick={() => listing.refetch()}
          >
            <RefreshCw />
          </IconButton>
        </div>
        <div className="directory-quick-results" aria-busy={busy}>
          {(!runner.online || listing.error || error) && (
            <p className="error directory-state" role="alert">
              {!runner.online ? '设备离线，无法浏览。' : error || listing.error?.message}
            </p>
          )}
          {listing.isPending ? (
            <p className="quiet directory-state">正在读取目录…</p>
          ) : (
            <div id={listId} role="listbox" aria-label="目录" className="directory-quick-list">
              {entries.map((entry, index) => (
                <div
                  className={`directory-quick-row ${active === index ? 'selected' : ''}`}
                  key={entry.path}
                >
                  <button
                    type="button"
                    role="option"
                    id={`${listId}-${index}`}
                    aria-selected={active === index}
                    disabled={selecting}
                    onClick={() => {
                      queued.current = null
                      setActive(index)
                      setError('')
                      setError('')
                      input.current?.focus()
                    }}
                    onDoubleClick={() => browse(entry.path)}
                  >
                    <Folder size={15} />
                    <span className="mono" title={entry.path}>
                      {entry.name}
                    </span>
                  </button>
                  <IconButton
                    label={`打开 ${entry.name}`}
                    disabled={selecting}
                    onClick={() => browse(entry.path)}
                  >
                    <ChevronRight />
                  </IconButton>
                </div>
              ))}
              {!entries.length && !listing.error && (
                <p className="quiet directory-state">{needle ? '无匹配目录' : '没有子目录'}</p>
              )}
            </div>
          )}
          {listing.hasNextPage && (
            <Button
              className="directory-load"
              loading={listing.isFetchingNextPage}
              disabled={selecting}
              onClick={() => listing.fetchNextPage()}
            >
              加载更多
            </Button>
          )}
        </div>
      </div>
      <div className="modal-footer directory-quick-footer">
        <output className="mono directory-current" title={selected ?? ''}>
          {selected ?? ''}
        </output>
        <Button disabled={selecting} onClick={onCancel}>
          取消
        </Button>
        <Button
          variant="solid"
          loading={selecting}
          disabled={!selected || busy || !!listing.error || !runner.online}
          onClick={choose}
        >
          选择文件夹
        </Button>
      </div>
    </>
  )
}
