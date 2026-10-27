import { t } from '../i18n'
import { parseJsonDocument } from './document'
import type { Definition, TemplateIssue } from './types'

export function definitionFrom(source: string): Definition {
  const value = parseJsonDocument(source) as unknown as Definition
  if (
    !value ||
    typeof value !== 'object' ||
    !value.task ||
    typeof value.task !== 'object' ||
    !value.inputs ||
    !value.bindings ||
    typeof value.bindings !== 'object' ||
    !value.inputs.properties ||
    typeof value.inputs.properties !== 'object'
  )
    throw new Error('taskTemplate.invalidDefinition')
  return value
}

export function issueText(issue: TemplateIssue): string {
  const key = 'taskTemplate.issue.' + issue.code,
    translated = t(key, issue.params)
  return translated === key ? issue.message : translated
}
