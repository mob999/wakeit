import { useRef, useState, useLayoutEffect } from 'react'
import * as Menu from '@radix-ui/react-dropdown-menu'
import { Braces, ChevronRight, ArrowUpRight } from 'lucide-react'
import { type Dag, type DagNode, type Runner, type DagIssue } from '../api'
import { t } from '../i18n'
import { Field, IconButton } from '../ui'
import { ChoicePicker } from '../ChoicePicker'
import { AgentLogo } from '../AgentLogo'
import { DirectoryPicker } from '../DirectoryPicker'
import { ModelPicker, ParameterFields } from '../ModelPicker'
import { SessionBinding } from '../sessions/SessionBinding'
import { useEditorValue } from '../EditorMemory'
import { MultiPicker } from './MultiPicker'
import { ancestors } from './model'
import { nodeName, nodeOptions, shortId } from './identity'
import { markFields } from './fields'
export function NodeForm({
  node,
  dag,
  runners,
  disabled,
  errors,
  onChange,
  onLocate,
}: {
  node: DagNode
  dag: Dag
  runners: Runner[]
  disabled: boolean
  errors: DagIssue[]
  onChange: (next: DagNode) => void
  onLocate: (id: string) => void
}) {
  const [modelOptions, setModelOptions] = useState<Record<string, string[]>>({})
  const [advanced, setAdvanced] = useEditorValue('advanced', false)
  const [selection, setSelection] = useEditorValue<[number, number]>('selection', [0, 0])
  const [referenceOpen, setReferenceOpen] = useState(false)
  const prompt = useRef<HTMLTextAreaElement>(null),
    form = useRef<HTMLFieldSetElement>(null)
  const runner = runners.find((r) => r.id === node.runner || r.name === node.runner),
    agent = runner?.agents.find((a) => a.id === node.agent)
  const upstream = ancestors(dag, node.id)
  const referenced = new Set(
    Array.from(node.prompt.matchAll(/\{\{\s*nodes\.([a-zA-Z0-9_-]+)\.result\s*\}\}/g)).map(
      (m) => m[1],
    ),
  )
  const patch = (value: Partial<DagNode>) => onChange({ ...node, ...value })
  useLayoutEffect(() => {
    markFields(
      form.current,
      errors.filter((e) => e.node_id === node.id),
    )
  })
  useLayoutEffect(() => {
    prompt.current?.setSelectionRange(...selection)
  }, [node.id])
  return (
    <fieldset
      ref={form}
      disabled={disabled}
      className="editor-fields dag-config-body"
      data-node-editor={node.id}
    >
      <Field label={t('dag.field.name')} fieldKey="name">
        <input
          aria-label={t('dag.field.name')}
          value={node.name}
          onChange={(e) => patch({ name: e.target.value })}
        />
      </Field>
      <Field label={t('dag.pre')} fieldKey="pre">
        <MultiPicker
          label={t('dag.pre')}
          disabled={disabled}
          value={node.pre}
          empty={t('dag.root')}
          chips
          options={nodeOptions(dag.nodes.filter((n) => n.id !== node.id)).map((option) => ({
            ...option,
            disabled: ancestors(dag, option.id).has(node.id),
            reason: ancestors(dag, option.id).has(node.id) ? t('dag.cycle') : undefined,
          }))}
          onChange={(pre) => patch({ pre })}
        />
      </Field>
      <div className="field dag-instruction" data-field="prompt">
        <div className="dag-prompt-head">
          <label htmlFor={'prompt-' + node.id}>{t('指令')}</label>
          <Menu.Root open={referenceOpen && !disabled} onOpenChange={setReferenceOpen}>
            <Menu.Trigger asChild>
              <IconButton label={t('dag.insertResult')} disabled={disabled || !upstream.size}>
                <Braces />
              </IconButton>
            </Menu.Trigger>
            <Menu.Portal>
              <Menu.Content className="dropdown dag-reference-menu" align="end">
                {dag.nodes
                  .filter((n) => upstream.has(n.id))
                  .map((n) => (
                    <Menu.Item
                      key={n.id}
                      onSelect={() => {
                        const field = prompt.current,
                          start = field?.selectionStart ?? node.prompt.length,
                          end = field?.selectionEnd ?? start,
                          token = '{{ nodes.' + n.id + '.result }}'
                        patch({
                          prompt: node.prompt.slice(0, start) + token + node.prompt.slice(end),
                        })
                        requestAnimationFrame(() => {
                          field?.focus()
                          field?.setSelectionRange(start + token.length, start + token.length)
                        })
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
        </div>
        <div>
          <textarea
            ref={prompt}
            id={'prompt-' + node.id}
            rows={6}
            value={node.prompt}
            onSelect={(e) =>
              setSelection([e.currentTarget.selectionStart, e.currentTarget.selectionEnd])
            }
            onChange={(e) => patch({ prompt: e.target.value })}
          />
          {referenced.size > 0 && (
            <div className="dag-reference-rows">
              {Array.from(referenced).map((id) => (
                <div key={id}>
                  <span>{nodeName(dag.nodes.find((n) => n.id === id) ?? { name: id })}</span>
                  <code>{'{{ nodes.' + id + '.result }}'}</code>
                  <IconButton
                    label={t('dag.locateSource', {
                      name: nodeName(dag.nodes.find((n) => n.id === id) ?? { name: id }),
                    })}
                    disabled={!dag.nodes.some((n) => n.id === id)}
                    onClick={() => onLocate(id)}
                  >
                    <ArrowUpRight />
                  </IconButton>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
      <Field label={t('Runner')} fieldKey="runner">
        <ChoicePicker
          label={t('Runner')}
          searchable
          disabled={disabled}
          value={node.runner}
          selectionLabel={runner?.name ?? (node.runner ? t('dag.deviceUnavailable') : undefined)}
          options={runners
            .filter((r) => !r.revoked)
            .map((r) => ({ value: r.id, label: r.name, status: r.online ? undefined : t('离线') }))}
          onChange={(id) => {
            if (id !== runner?.id)
              patch({ runner: id, agent: '', cwd: '', parameters: {}, session: null })
          }}
        />
      </Field>
      <Field label={t('Agent')} fieldKey="agent">
        <ChoicePicker
          label={t('Agent')}
          searchable
          disabled={disabled || !runner}
          value={node.agent}
          selectionLabel={(agent?.name ?? node.agent) || undefined}
          options={(runner?.agents ?? [])
            .filter((a) => a.available)
            .map((a) => ({
              value: a.id,
              label: a.name,
              keywords: a.id,
              icon: <AgentLogo name={a.name} logo={a.logo} />,
            }))}
          onChange={(id) => {
            if (id !== node.agent) patch({ agent: id, parameters: {}, session: null })
          }}
        />
      </Field>
      <DirectoryPicker
        disabled={disabled}
        runner={runner}
        value={node.cwd}
        onChange={(cwd) => {
          if (cwd !== node.cwd) patch({ cwd, session: null })
        }}
      />
      <ModelPicker
        runner={runner}
        agent={agent}
        cwd={node.cwd}
        values={node.parameters}
        onOptionsChange={setModelOptions}
        submitting={disabled}
        onChange={(parameters) => patch({ parameters })}
      />
      <details
        className="dag-advanced"
        open={advanced}
        onToggle={(e) => setAdvanced(e.currentTarget.open)}
      >
        <summary>
          <ChevronRight size={14} />
          {t('高级设置')}
        </summary>
        <div>
          <Field label={t('执行超时')} fieldKey="timeout">
            <input
              aria-label={t('执行超时')}
              value={node.timeout}
              onChange={(e) => patch({ timeout: e.target.value })}
            />
          </Field>
          {(agent?.session_supported || node.session) && (
            <SessionBinding
              runner={runner}
              agent={agent}
              cwd={node.cwd}
              disabled={disabled}
              value={node.session ?? null}
              onChange={(session) => patch({ session })}
            />
          )}
          <ParameterFields
            agent={agent}
            values={node.parameters}
            options={modelOptions}
            disabled={disabled}
            onChange={(parameters) => patch({ parameters })}
          />
        </div>
      </details>
    </fieldset>
  )
}
