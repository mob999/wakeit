import { type DagIssue } from '../api/types'
export function issueField(issue: DagIssue) {
  if (!issue.node_id) return issue.path
  return issue.path.replace(/^dag\.nodes(?:\[[^\]]+\]|\.[^.]+)\./, '')
}
export function locateField(root: HTMLElement | null, issue: DagIssue) {
  const scope = issue.node_id
    ? root?.querySelector<HTMLElement>('[data-node-editor="' + issue.node_id + '"]')
    : root
  const key = issueField(issue),
    fields = Array.from(scope?.querySelectorAll<HTMLElement>('[data-field]') ?? [])
  const field =
    fields.find((f) => f.dataset.field === key) ??
    fields.find((f) => key.startsWith(f.dataset.field + '.'))
  const details = field?.closest('details')
  if (details) details.open = true
  const control =
    field?.querySelector<HTMLElement>('input, textarea, select') ??
    field?.querySelector<HTMLElement>('button')
  control?.focus({ preventScroll: true })
  field?.scrollIntoView({ block: 'nearest' })
}
export function markFields(root: HTMLElement | null, issues: DagIssue[]) {
  root?.querySelectorAll<HTMLElement>('[data-field]').forEach((field) => {
    if (root?.classList.contains('dag-editor') && field.closest('[data-node-editor]')) return
    const invalid = issues.some((issue) => {
      const key = issueField(issue)
      return key === field.dataset.field || key.startsWith(field.dataset.field + '.')
    })
    field.dataset.invalid = String(invalid)
    field
      .querySelectorAll<HTMLElement>(
        'input,textarea,button[role=combobox],button.directory-selector',
      )
      .forEach((control) => control.setAttribute('aria-invalid', String(invalid)))
  })
}
