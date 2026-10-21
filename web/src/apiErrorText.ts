type Translate = (key: string, values?: Record<string, unknown>) => string

// Translate Gateway diagnostics at the UI boundary; never rewrite Agent output.
export function apiErrorText(
  message: string,
  translate: Translate,
  stateName: (state: string) => string,
): string {
  const state = /^Node in state ([a-z_]+) cannot be retried$/.exec(message)
  if (state) return translate('retry.error.state', { state: stateName(state[1]) })
  for (const [pattern, key, field] of [
    [/^Upstream result unavailable: (.+)$/s, 'retry.error.resultUnavailable', 'node'],
    [
      /^Upstream result missing, truncated or unavailable: (.+)$/s,
      'retry.error.resultIncomplete',
      'node',
    ],
    [/^Upstream result is empty: (.+)$/s, 'retry.error.resultEmpty', 'node'],
    [/^Invalid result reference: (.+)$/s, 'retry.error.reference', 'reference'],
    [/^invalid value for parameter (.+)$/s, 'retry.error.parameterValue', 'parameter'],
    [/^parameter (.+) exceeds 4096 bytes$/s, 'retry.error.parameterSize', 'parameter'],
  ] as const) {
    const match = pattern.exec(message)
    if (match) return translate(key, { [field]: match[1] })
  }
  return translate(message)
}
