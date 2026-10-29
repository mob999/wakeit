import type { Dag, TaskConfig } from '../api/types'
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json }
export type InputSchema = {
  type: string | string[]
  title?: string | null
  description?: string | null
  format?: string | null
  properties?: Record<string, InputSchema>
  required?: string[]
  additionalProperties?: false
  items?: InputSchema | null
  enum?: Json[] | null
  default?: Json
  minimum?: number | null
  maximum?: number | null
  minLength?: number | null
  maxLength?: number | null
  minItems?: number | null
  maxItems?: number | null
}
export type Binding = {
  runner?: string | null
  agent?: string | null
  parameters?: Record<string, string> | null
}
export type TaskBlueprint = Record<string, unknown> & {
  schedule?: TaskConfig['schedule']
  dag?: Record<string, unknown>
}
export type Definition = {
  version: 1
  id: string
  name: string
  description?: string
  inputs: InputSchema
  bindings: Record<string, { title?: string; defaults?: Binding }>
  task: TaskBlueprint
}
export type TemplateRecord = {
  id: string
  revision: number
  definition: Definition
  archived_at: string | null
  created_at?: string
  updated_at?: string
}
export type Overrides = { values: Record<string, Json>; unset: string[] }
export type Request = Overrides & {
  expected_revision: number
  bindings: Record<string, Binding>
  task_id?: string
  name?: string
  schedule?: TaskConfig['schedule']
  request_id?: string
  run_now?: boolean
}
export type TemplateIssue = {
  code: string
  path: string
  message: string
  params?: Record<string, unknown>
  node_id?: string
  line?: number
  column?: number
}
export type Preview = {
  template_revision: number
  config: TaskConfig | null
  graph: Dag | null
  resolved_inputs: Record<string, Json>
  used_bindings: string[]
  source_map: Record<
    string,
    { declaration: string; binding: string; group?: string; key?: string; input_path?: string }
  >
  issues: TemplateIssue[]
}
