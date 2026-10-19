import { useRef, useState } from 'react'
import { Braces, ChevronRight } from 'lucide-react'
import { type Dag, type DagNode, type Runner, type DagIssue } from '../api'
import { t } from '../i18n'
import { Field, IconButton } from '../ui'
import { ChoicePicker } from '../ChoicePicker'
import { AgentLogo } from '../AgentLogo'
import { DirectoryPicker } from '../DirectoryPicker'
import { ModelPicker, ParameterFields } from '../ModelPicker'
import { SessionBinding } from '../sessions/SessionBinding'
import { MultiPicker } from './MultiPicker'
import { ancestors } from './model'

export function NodeForm({
  node,
  dag,
  runners,
  disabled,
  errors,
  onChange,
}: {
  node: DagNode
  dag: Dag
  runners: Runner[]
  disabled: boolean
  errors: DagIssue[]
  onChange: (next: DagNode) => void
}) {
  const [modelOptions, setModelOptions] = useState<Record<string, string[]>>({})
  const [referenceOpen, setReferenceOpen] = useState(false)
  const prompt = useRef<HTMLTextAreaElement>(null)
  const runner = runners.find((r) => r.id === node.runner || r.name === node.runner)
  const agent = runner?.agents.find((a) => a.id === node.agent)
  const upstream = ancestors(dag, node.id)
  const patch = (value: Partial<DagNode>) => onChange({ ...node, ...value })
  const error = (field: string) =>
    errors.find((e) => e.node_id === node.id && e.path.endsWith(`.${field}`))
  const hint = (field: string) =>
    error(field) && (
      <small className="error" role="alert">
        {error(field)!.message}
      </small>
    )
  return (
    <fieldset disabled={disabled} className="editor-fields dag-config-body">
      <Field label={t('dag.field.name')}>
        <input
          aria-label={t('dag.field.name')}
          autoFocus
          value={node.name}
          aria-invalid={!!error('name')}
          onChange={(e) => patch({ name: e.target.value })}
        />
        {hint('name')}
      </Field>
      <Field label={t('dag.pre')}>
        <MultiPicker
          label={t('dag.pre')}
          disabled={disabled}
          value={node.pre}
          empty={t('dag.root')}
          options={dag.nodes
            .filter((n) => n.id !== node.id)
            .map((n) => ({
              id: n.id,
              name: n.name || t('dag.unnamed'),
              disabled: ancestors(dag, n.id).has(node.id),
              reason: ancestors(dag, n.id).has(node.id) ? t('dag.cycle') : undefined,
            }))}
          onChange={(pre) => patch({ pre })}
        />
        {hint('pre')}
      </Field>
      <div className="field">
        <div className="dag-prompt-head">
          <label htmlFor={`prompt-${node.id}`}>{t('指令')}</label>
          <ChoicePicker
            label={t('dag.insertResult')}
            value=""
            disabled={disabled || !upstream.size}
            open={referenceOpen}
            onOpenChange={setReferenceOpen}
            placeholder={t('dag.insertResult')}
            options={dag.nodes
              .filter((n) => upstream.has(n.id))
              .map((n) => ({ value: n.id, label: n.name || t('dag.unnamed') }))}
            onChange={(id) => {
              const field = prompt.current
              const start = field?.selectionStart ?? node.prompt.length
              const end = field?.selectionEnd ?? start
              const token = `{{ nodes.${id}.result }}`
              patch({ prompt: node.prompt.slice(0, start) + token + node.prompt.slice(end) })
              requestAnimationFrame(() => {
                field?.focus()
                field?.setSelectionRange(start + token.length, start + token.length)
              })
            }}
          />
        </div>
        <textarea
          ref={prompt}
          id={`prompt-${node.id}`}
          rows={7}
          value={node.prompt}
          aria-invalid={!!error('prompt')}
          onChange={(e) => patch({ prompt: e.target.value })}
        />
        {hint('prompt')}
      </div>
      <div className="form-grid">
        <Field label={t('Runner')}>
          <ChoicePicker
            label={t('Runner')}
            searchable
            disabled={disabled}
            value={node.runner}
            selectionLabel={(runner?.name ?? node.runner) || undefined}
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
          {hint('runner')}
        </Field>
        <Field label={t('Agent')}>
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
          {hint('agent')}
        </Field>
      </div>
      <DirectoryPicker
        runner={runner}
        value={node.cwd}
        onChange={(cwd) => patch({ cwd, session: null })}
      />
      {hint('cwd')}
      <ModelPicker
        runner={runner}
        agent={agent}
        cwd={node.cwd}
        values={node.parameters}
        onOptionsChange={setModelOptions}
        submitting={disabled}
        onChange={(parameters) => patch({ parameters })}
      />
      <details className="advanced-settings">
        <summary>
          <ChevronRight size={14} />
          {t('高级设置')}
        </summary>
        <div className="advanced-body">
          <Field label={t('执行超时')}>
            <input
              aria-label={t('执行超时')}
              value={node.timeout}
              onChange={(e) => patch({ timeout: e.target.value })}
            />
            {hint('timeout')}
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
