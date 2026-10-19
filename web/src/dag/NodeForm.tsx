import { useRef, useState, useLayoutEffect } from 'react'
import { ChevronRight } from 'lucide-react'
import { type Dag, type DagNode, type Runner, type DagIssue } from '../api'
import { t } from '../i18n'
import { Field } from '../ui'
import { ChoicePicker } from '../ChoicePicker'
import { AgentLogo } from '../AgentLogo'
import { DirectoryPicker } from '../DirectoryPicker'
import { ModelPicker, ParameterFields } from '../ModelPicker'
import { SessionBinding } from '../sessions/SessionBinding'
import { useEditorValue } from '../EditorMemory'
import { MultiPicker } from './MultiPicker'
import { ancestors } from './model'
import { NodePrompt } from './NodePrompt'
import { nodeOptions } from './identity'
import { markFields } from './fields'
export function NodeForm({
  node,
  dag,
  runners,
  disabled,
  errors,
  onChange,
  onLocate,
  focusTarget,
}: {
  node: DagNode
  dag: Dag
  runners: Runner[]
  disabled: boolean
  errors: DagIssue[]
  onChange: (next: DagNode) => void
  onLocate: (id: string) => void
  focusTarget?: HTMLElement | null
}) {
  const [modelOptions, setModelOptions] = useState<Record<string, string[]>>({})
  const [advanced, setAdvanced] = useEditorValue('advanced', false)
  const form = useRef<HTMLFieldSetElement>(null)
  const runner = runners.find((r) => r.id === node.runner || r.name === node.runner),
    agent = runner?.agents.find((a) => a.id === node.agent)
  const patch = (value: Partial<DagNode>) => onChange({ ...node, ...value })
  useLayoutEffect(() => {
    markFields(
      form.current,
      errors.filter((e) => e.node_id === node.id),
    )
  })
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
      <NodePrompt
        node={node}
        dag={dag}
        disabled={disabled}
        focusTarget={focusTarget}
        onChange={(prompt) => patch({ prompt })}
        onLocate={onLocate}
      />
      <div className="dag-environment-fields">
        <Field label={t('Runner')} fieldKey="runner">
          <ChoicePicker
            label={t('Runner')}
            searchable
            disabled={disabled}
            value={node.runner}
            selectionLabel={runner?.name ?? (node.runner ? t('dag.deviceUnavailable') : undefined)}
            options={runners
              .filter((r) => !r.revoked)
              .map((r) => ({
                value: r.id,
                label: r.name,
                status: r.online ? undefined : t('离线'),
              }))}
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
      </div>
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
