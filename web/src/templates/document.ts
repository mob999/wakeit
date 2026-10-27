import { parseDocument } from 'yaml'
import type { Json, Overrides, InputSchema } from './types'
export function parseJsonDocument(source: string): Json {
  const doc = parseDocument(source, { uniqueKeys: true })
  if (doc.errors.length) throw new Error(doc.errors[0].message)
  function visit(value: unknown): asserts value is Json {
    if (value === null || typeof value === 'string' || typeof value === 'boolean') return
    if (
      typeof value === 'number' &&
      Number.isFinite(value) &&
      (!Number.isInteger(value) || Number.isSafeInteger(value))
    )
      return
    if (Array.isArray(value)) {
      value.forEach(visit)
      return
    }
    if (value && typeof value === 'object' && Object.getPrototypeOf(value) === Object.prototype) {
      Object.values(value).forEach(visit)
      return
    }
    throw new Error('taskTemplate.jsonOnly')
  }
  // Reject non-string keys, tags and aliases before YAML can coerce them.
  function nodes(node: any): void {
    if (!node || typeof node !== 'object') return
    if (node.tag || (node.source && node.constructor?.name === 'Alias'))
      throw new Error('taskTemplate.jsonOnly')
    if (node.constructor?.name === 'YAMLMap')
      for (const pair of node.items) {
        if (typeof pair.key?.value !== 'string') throw new Error('taskTemplate.jsonOnly')
        nodes(pair.value)
      }
    else if (node.items) node.items.forEach(nodes)
  }
  nodes(doc.contents)
  const value: unknown = doc.toJS({ maxAliasCount: 0 })
  visit(value)
  return value
}
export function parseOverrides(source: string): Overrides {
  const value = parseJsonDocument(source)
  if (
    !value ||
    typeof value !== 'object' ||
    Array.isArray(value) ||
    Object.keys(value).some((k) => !['values', 'unset'].includes(k))
  )
    throw new Error('taskTemplate.overrideEnvelope')
  const values = value.values ?? {},
    unset = value.unset ?? []
  if (
    !values ||
    typeof values !== 'object' ||
    Array.isArray(values) ||
    !Array.isArray(unset) ||
    !unset.every((v) => typeof v === 'string')
  )
    throw new Error('taskTemplate.overrideEnvelope')
  return { values, unset: unset as string[] }
}
export const types = (schema: InputSchema) =>
  Array.isArray(schema.type) ? schema.type : [schema.type]
export function emptyValue(schema: InputSchema): Json {
  switch (types(schema).find((t) => t !== 'null')) {
    case 'object':
      return Object.fromEntries(
        (schema.required ?? []).map((k) => [k, emptyValue(schema.properties![k])]),
      )
    case 'array':
      return []
    case 'boolean':
      return false
    case 'integer':
    case 'number':
      return schema.minimum ?? 0
    case 'null':
      return null
    default:
      return schema.enum?.[0] ?? ''
  }
}
