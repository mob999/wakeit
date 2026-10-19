import { useLayoutEffect, useRef, useState } from 'react'
import * as Dialog from '@radix-ui/react-dialog'
import * as Menu from '@radix-ui/react-dropdown-menu'
import { Braces, ArrowUpRight, Maximize2, X } from 'lucide-react'
import { t } from '../i18n'
import { IconButton } from '../ui'
import { useEditorValue } from '../EditorMemory'
import type { Dag, DagNode } from '../api'
import { ancestors } from './model'
import { nodeName, shortId } from './identity'
export function NodePrompt({
  node,
  dag,
  disabled,
  focusTarget,
  onChange,
  onLocate,
}: {
  node: DagNode
  dag: Dag
  disabled: boolean
  focusTarget?: HTMLElement | null
  onChange: (prompt: string) => void
  onLocate: (id: string) => void
}) {
  const [expanded, setExpanded] = useState(false),
    [referenceOpen, setReferenceOpen] = useState(false)
  const [selection, setSelection] = useEditorValue<[number, number]>('selection', [0, 0])
  const inline = useRef<HTMLTextAreaElement>(null),
    large = useRef<HTMLTextAreaElement>(null),
    cursor = useRef(selection),
    insertFocus = useRef<number | null>(null)
  useLayoutEffect(() => {
    if (!expanded) inline.current?.setSelectionRange(...cursor.current)
  }, [node.id, expanded])
  const upstream = ancestors(dag, node.id),
    referenced = new Set(
      Array.from(node.prompt.matchAll(/\{\{\s*nodes\.([a-zA-Z0-9_-]+)\.result\s*\}\}/g)).map(
        (m) => m[1],
      ),
    )
  function textarea(big: boolean) {
    return (
      <textarea
        ref={big ? large : inline}
        id={(big ? 'large-prompt-' : 'prompt-') + node.id}
        aria-label={t('指令')}
        disabled={disabled}
        rows={big ? 18 : 8}
        value={node.prompt}
        onSelect={(event) => {
          const value: [number, number] = [
            event.currentTarget.selectionStart,
            event.currentTarget.selectionEnd,
          ]
          cursor.current = value
          setSelection(value)
        }}
        onBlur={(event) => {
          cursor.current = [event.currentTarget.selectionStart, event.currentTarget.selectionEnd]
          setSelection(cursor.current)
        }}
        onChange={(event) => onChange(event.target.value)}
      />
    )
  }
  function references() {
    return (
      <Menu.Root open={referenceOpen && !disabled} onOpenChange={setReferenceOpen}>
        <Menu.Trigger asChild>
          <IconButton label={t('dag.insertResult')} disabled={disabled || !upstream.size}>
            <Braces />
          </IconButton>
        </Menu.Trigger>
        <Menu.Portal>
          <Menu.Content
            className="dropdown dag-reference-menu"
            align="end"
            onCloseAutoFocus={(event) => {
              if (insertFocus.current != null) {
                event.preventDefault()
                const field = expanded ? large.current : inline.current
                field?.focus()
                field?.setSelectionRange(insertFocus.current, insertFocus.current)
                insertFocus.current = null
              }
            }}
          >
            {dag.nodes
              .filter((n) => upstream.has(n.id))
              .map((n) => (
                <Menu.Item
                  key={n.id}
                  onSelect={() => {
                    const field = expanded ? large.current : inline.current,
                      start = field?.selectionStart ?? node.prompt.length,
                      end = field?.selectionEnd ?? start,
                      token = '{{ nodes.' + n.id + '.result }}'
                    onChange(node.prompt.slice(0, start) + token + node.prompt.slice(end))
                    insertFocus.current = start + token.length
                    cursor.current = [insertFocus.current, insertFocus.current]
                    setSelection(cursor.current)
                  }}
                >
                  <span>
                    {nodeName(n)}
                    <small className="quiet mono">{shortId(n.id)}</small>
                  </span>
                </Menu.Item>
              ))}
          </Menu.Content>
        </Menu.Portal>
      </Menu.Root>
    )
  }
  return (
    <div className="field dag-instruction" data-field="prompt">
      <div className="dag-prompt-head">
        <label htmlFor={'prompt-' + node.id}>{t('指令')}</label>
        <div className="dag-prompt-actions">
          {!expanded && references()}
          <IconButton
            label={t('dag.expandPrompt')}
            disabled={disabled}
            onClick={() => {
              cursor.current = inline.current
                ? [inline.current.selectionStart, inline.current.selectionEnd]
                : cursor.current
              setExpanded(true)
            }}
          >
            <Maximize2 />
          </IconButton>
        </div>
      </div>
      <div>
        {!expanded && textarea(false)}
        {referenced.size > 0 && (
          <div className="dag-reference-rows">
            {[...referenced].map((id) => (
              <div key={id}>
                <span>{nodeName(dag.nodes.find((n) => n.id === id) ?? { name: id })}</span>
                <code>{'{{ nodes.' + id + '.result }}'}</code>
                <IconButton
                  label={t('dag.locateSource', {
                    name: nodeName(dag.nodes.find((n) => n.id === id) ?? { name: id }),
                  })}
                  disabled={disabled || !dag.nodes.some((n) => n.id === id)}
                  onClick={() => onLocate(id)}
                >
                  <ArrowUpRight />
                </IconButton>
              </div>
            ))}
          </div>
        )}
      </div>
      <Dialog.Root
        open={expanded}
        onOpenChange={(open) => {
          if (!disabled) {
            setExpanded(open)
            setReferenceOpen(false)
          }
        }}
      >
        <Dialog.Portal container={focusTarget ?? undefined}>
          <Dialog.Overlay className="dag-prompt-overlay" />
          <Dialog.Content
            className="dag-prompt-focus content-card"
            aria-describedby={undefined}
            onOpenAutoFocus={(event) => {
              event.preventDefault()
              large.current?.focus()
              large.current?.setSelectionRange(...cursor.current)
            }}
            onCloseAutoFocus={(event) => {
              event.preventDefault()
              requestAnimationFrame(() => {
                inline.current?.focus({ preventScroll: true })
                inline.current?.setSelectionRange(...cursor.current)
              })
            }}
          >
            <header className="card-header">
              <Dialog.Title>{nodeName(node)}</Dialog.Title>
              {expanded && references()}
              <Dialog.Close asChild>
                <IconButton label={t('关闭')} disabled={disabled}>
                  <X />
                </IconButton>
              </Dialog.Close>
            </header>
            <div className="dag-prompt-focus-body">
              <label htmlFor={'large-prompt-' + node.id}>{t('指令')}</label>
              {textarea(true)}
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </div>
  )
}
