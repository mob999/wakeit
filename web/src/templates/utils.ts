import { t } from '../i18n'
import type { TemplateIssue } from './types'

export function issueText(issue: TemplateIssue): string {
  const key = 'taskTemplate.issue.' + issue.code,
    translated = t(key, issue.params)
  return translated === key ? issue.message : translated
}
