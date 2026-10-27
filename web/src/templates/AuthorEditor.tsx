import YAML from 'yaml'
import { useState } from 'react'
import { t } from '../i18n'
import { Button } from '../ui'
import type { Definition, InputSchema } from './types'
export function starter(dag: boolean): Definition {
  const inputs: InputSchema = {
    type: 'object',
    additionalProperties: false,
    required: ['directory', ...(dag ? ['repositories'] : ['repository'])],
    properties: {
      directory: { type: 'string', format: 'directory', title: t('工作目录') },
      ...(dag
        ? {
            repositories: {
              type: 'array',
              title: t('taskTemplate.repositories'),
              items: {
                type: 'object',
                additionalProperties: false,
                required: ['key', 'url'],
                properties: {
                  key: { type: 'string', minLength: 1 },
                  url: { type: 'string', format: 'repository-url' },
                  enabled: { type: 'boolean' },
                },
              },
              default: [
                { key: 'example', url: 'https://github.com/example/repository.git', enabled: true },
              ],
            },
          }
        : {
            repository: {
              type: 'string',
              format: 'repository-url',
              title: t('taskTemplate.repository'),
            },
          }),
      include_diff: { type: 'boolean', default: false },
    },
  }
  return {
    version: 1,
    id: crypto.randomUUID(),
    name: t('taskTemplate.newTemplate'),
    description: '',
    inputs,
    bindings: { worker: { title: t('taskTemplate.environment') } },
    task: {
      name: {
        template: dag
          ? '[[ inputs.repositories | length ]] ' + t('taskTemplate.repositories')
          : '[[ inputs.repository ]]',
      },
      schedule: null,
      enabled: true,
      execution: {
        timeout: dag ? null : '1h',
        start_grace: '1h',
        overlap: 'skip',
        misfire: 'skip',
      },
      ...(dag
        ? {
            dag: {
              nodes: [
                {
                  group: 'review',
                  for_each: { expr: 'inputs.repositories' },
                  as: 'repo',
                  key: 'key',
                  when: { expr: 'repo.enabled | default(true)' },
                  node: {
                    name: { template: '[[ repo.key ]]' },
                    binding: 'worker',
                    cwd: { template: '[[ inputs.directory ]]' },
                    prompt: { parts: [{ template: t('taskTemplate.exampleInstructionDag') }] },
                  },
                },
                {
                  id: 'summary',
                  name: t('taskTemplate.summary'),
                  binding: 'worker',
                  cwd: { template: '[[ inputs.directory ]]' },
                  pre: [{ group: 'review', all: true }],
                  prompt: {
                    parts: [
                      { text: t('taskTemplate.exampleSummary') },
                      { result: { group: 'review', all: true }, separator: '\n\n' },
                    ],
                  },
                },
              ],
              results: [{ node: 'summary' }],
            },
          }
        : {
            binding: 'worker',
            cwd: { template: '[[ inputs.directory ]]' },
            prompt: { template: t('taskTemplate.exampleInstruction') },
          }),
    },
  }
}
// Only the declared text fields are editable here; result selectors are retained.
export function textFields(
  definition: Definition,
): { path: (string | number)[]; value: string; template: boolean }[] {
  const result: { path: (string | number)[]; value: string; template: boolean }[] = []
  function text(value: any, path: (string | number)[]) {
    if (typeof value === 'string') result.push({ path, value, template: false })
    else if (value && typeof value.template === 'string')
      result.push({ path, value: value.template, template: true })
  }
  for (const key of ['name', 'cwd', 'prompt']) text(definition.task?.[key], ['task', key])
  const nodes = (definition.task?.dag as any)?.nodes
  if (Array.isArray(nodes))
    nodes.forEach((decl: any, i: number) => {
      const base: (string | number)[] = [
          'task',
          'dag',
          'nodes',
          i,
          ...(decl?.node ? ['node'] : []),
        ],
        node = decl?.node ?? decl
      for (const key of ['name', 'cwd']) text(node?.[key], [...base, key])
      if (Array.isArray(node?.prompt?.parts))
        node.prompt.parts.forEach((part: any, j: number) => {
          if (typeof part?.text === 'string')
            result.push({
              path: [...base, 'prompt', 'parts', j],
              value: part.text,
              template: false,
            })
          else if (typeof part?.template === 'string')
            result.push({
              path: [...base, 'prompt', 'parts', j],
              value: part.template,
              template: true,
            })
        })
    })
  return result
}
export function replaceText(
  def: Definition,
  path: (string | number)[],
  value: string,
  template: boolean,
): Definition {
  const next = structuredClone(def)
  let target: any = next
  for (const key of path.slice(0, -1)) target = target[key]
  const part = path.at(-2) === 'parts'
  target[path.at(-1)!] = template ? { template: value } : part ? { text: value } : value
  return next
}
export function AuthorEditor({
  source,
  definition,
  onSource,
  error,
}: {
  source: string
  definition: Definition | null
  onSource: (source: string) => void
  error: string
}) {
  const [view, setView] = useState<'source' | 'fields'>('source'),
    [newName, setNewName] = useState(''),
    [newType, setNewType] = useState('string')
  const apply = (def: Definition) => onSource(YAML.stringify(def, { lineWidth: 0 }))
  return (
    <div className="template-author">
      <div className="template-toolbar" role="group" aria-label={t('taskTemplate.authorView')}>
        <Button aria-pressed={view === 'source'} onClick={() => setView('source')}>
          YAML / JSON
        </Button>
        <Button aria-pressed={view === 'fields'} onClick={() => setView('fields')}>
          {t('taskTemplate.fields')}
        </Button>
      </div>
      {view === 'source' ? (
        <textarea
          className="template-source mono"
          spellCheck={false}
          aria-label={t('taskTemplate.definition')}
          value={source}
          onChange={(e) => onSource(e.target.value)}
        />
      ) : definition ? (
        <>
          <label>
            {t('名称')}
            <input
              value={definition.name}
              onChange={(e) => apply({ ...definition, name: e.target.value })}
            />
          </label>
          <label>
            {t('taskTemplate.description')}
            <input
              value={definition.description ?? ''}
              onChange={(e) => apply({ ...definition, description: e.target.value })}
            />
          </label>
          <h4>{t('taskTemplate.inputSchema')}</h4>
          {Object.entries(definition.inputs?.properties ?? {}).map(([name, spec]) => (
            <div className="template-schema-row" key={name}>
              <code>{name}</code>
              <span>{Array.isArray(spec.type) ? spec.type.join(' | ') : spec.type}</span>
              <input
                aria-label={t('taskTemplate.inputTitle', { name })}
                placeholder={name}
                value={spec.title ?? ''}
                onChange={(e) =>
                  apply({
                    ...definition,
                    inputs: {
                      ...definition.inputs,
                      properties: {
                        ...definition.inputs.properties,
                        [name]: { ...spec, title: e.target.value },
                      },
                    },
                  })
                }
              />
              <label>
                <input
                  type="checkbox"
                  checked={definition.inputs.required?.includes(name) ?? false}
                  onChange={(e) =>
                    apply({
                      ...definition,
                      inputs: {
                        ...definition.inputs,
                        required: [
                          ...(definition.inputs.required ?? []).filter((k) => k !== name),
                          ...(e.target.checked ? [name] : []),
                        ],
                      },
                    })
                  }
                />
                {t('taskTemplate.required')}
              </label>
            </div>
          ))}
          <div className="template-schema-row">
            <input
              aria-label={t('taskTemplate.inputName')}
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
            />
            <select
              aria-label={t('类型')}
              value={newType}
              onChange={(e) => setNewType(e.target.value)}
            >
              {['string', 'boolean', 'integer', 'number', 'array', 'object'].map((k) => (
                <option key={k}>{k}</option>
              ))}
            </select>
            <Button
              disabled={!newName || !!definition.inputs.properties?.[newName]}
              onClick={() => {
                const spec: InputSchema = {
                  type: newType,
                  ...(newType === 'array'
                    ? { items: { type: 'string' } }
                    : newType === 'object'
                      ? { properties: {}, additionalProperties: false }
                      : {}),
                }
                apply({
                  ...definition,
                  inputs: {
                    ...definition.inputs,
                    properties: { ...definition.inputs.properties, [newName]: spec },
                  },
                })
                setNewName('')
              }}
            >
              {t('taskTemplate.addInput')}
            </Button>
          </div>
          <h4>{t('指令')}</h4>
          {textFields(definition).map((field) => (
            <label className="template-text-field" key={field.path.join('.')}>
              <div>
                <code>{field.path.join('.')}</code>
                <select
                  aria-label={t('taskTemplate.textMode', { name: field.path.join('.') })}
                  value={field.template ? 'template' : 'text'}
                  onChange={(e) =>
                    apply(
                      replaceText(
                        definition,
                        field.path,
                        field.value,
                        e.target.value === 'template',
                      ),
                    )
                  }
                >
                  <option value="text">{t('taskTemplate.text')}</option>
                  <option value="template">{t('taskTemplate.templateText')}</option>
                </select>
              </div>
              {field.template && (
                <div className="template-snippets">
                  <select
                    aria-label={t('taskTemplate.insertInput')}
                    value=""
                    onChange={(e) =>
                      apply(
                        replaceText(
                          definition,
                          field.path,
                          field.value + '[[ inputs.' + e.target.value + ' ]]',
                          true,
                        ),
                      )
                    }
                  >
                    <option value="" disabled>
                      {t('taskTemplate.insertInput')}
                    </option>
                    {Object.keys(definition.inputs.properties ?? {}).map((k) => (
                      <option key={k} value={k}>
                        {k}
                      </option>
                    ))}
                  </select>
                  <Button
                    onClick={() => {
                      const key =
                        Object.keys(definition.inputs.properties ?? {}).find(
                          (k) => definition.inputs.properties![k].type === 'boolean',
                        ) ?? 'condition'
                      apply(
                        replaceText(
                          definition,
                          field.path,
                          field.value + '\n{% if inputs.' + key + ' %}\n\n{% endif %}',
                          true,
                        ),
                      )
                    }}
                  >
                    {t('taskTemplate.conditionSnippet')}
                  </Button>
                  <Button
                    onClick={() => {
                      const key =
                        Object.keys(definition.inputs.properties ?? {}).find(
                          (k) => definition.inputs.properties![k].type === 'array',
                        ) ?? 'items'
                      apply(
                        replaceText(
                          definition,
                          field.path,
                          field.value +
                            '\n{% for item in inputs.' +
                            key +
                            ' %}\n[[ item | tojson ]]\n{% endfor %}',
                          true,
                        ),
                      )
                    }}
                  >
                    {t('taskTemplate.loopSnippet')}
                  </Button>
                </div>
              )}
              <textarea
                rows={field.path.includes('prompt') ? 5 : 2}
                value={field.value}
                onChange={(e) =>
                  apply(replaceText(definition, field.path, e.target.value, field.template))
                }
              />
            </label>
          ))}
        </>
      ) : null}
      {error && (
        <p className="template-error" role="alert">
          {t(error)}
        </p>
      )}
    </div>
  )
}
