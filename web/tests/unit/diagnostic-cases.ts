// Contract examples shared by routing and catalog tests; never derived from production patterns.
export const retryDiagnostics = [
  ['Node in state succeeded cannot be retried', 'retry.error.state', { state: 'state:succeeded' }],
  ['Upstream result unavailable: source_1', 'retry.error.resultUnavailable', { node: 'source_1' }],
  [
    'Upstream result missing, truncated or unavailable: report-2',
    'retry.error.resultIncomplete',
    { node: 'report-2' },
  ],
  ['Upstream result is empty: collect', 'retry.error.resultEmpty', { node: 'collect' }],
  [
    'Invalid result reference: collect.output',
    'retry.error.reference',
    { reference: 'collect.output' },
  ],
  [
    'invalid value for parameter reasoning',
    'retry.error.parameterValue',
    { parameter: 'reasoning' },
  ],
  ['parameter model exceeds 4096 bytes', 'retry.error.parameterSize', { parameter: 'model' }],
] as const

export const literalDiagnostics = [
  'Task is archived',
  'Execution changed; refresh before retrying',
  'Session is currently in use',
  'Resolve unknown executions before retrying',
] as const
