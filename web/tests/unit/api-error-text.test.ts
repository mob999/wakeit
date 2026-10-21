import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { apiErrorText } from '../../src/apiErrorText.ts'

for (const language of ['zh', 'en']) {
  const messages = JSON.parse(
    readFileSync(new URL(`../../src/locales/${language}.json`, import.meta.url), 'utf8'),
  ) as Record<string, string>
  const translate = (key: string, values?: Record<string, unknown>) =>
    (messages[key] ?? key).replace(/\{\{(\w+)\}\}/g, (_, name: string) =>
      String(values?.[name] ?? `{{${name}}}`),
    )
  const stateName = (state: string) => (state === 'succeeded' ? messages['成功'] : state)
  test(`${language}: retry diagnostics translate parameters without losing identifiers`, () => {
    for (const [source, key, values] of [
      [
        'Node in state succeeded cannot be retried',
        'retry.error.state',
        { state: stateName('succeeded') },
      ],
      [
        'Upstream result unavailable: source_1',
        'retry.error.resultUnavailable',
        { node: 'source_1' },
      ],
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
    ] as const) {
      assert.ok(messages[key])
      const result = apiErrorText(source, translate, stateName)
      assert.equal(result, translate(key, values))
      assert.doesNotMatch(result, /\{\{|retry\.error\./)
    }
    for (const source of [
      'Task is archived',
      'Execution changed; refresh before retrying',
      'Session is currently in use',
      'Resolve unknown executions before retrying',
    ]) {
      assert.ok(messages[source])
      assert.equal(apiErrorText(source, translate, stateName), messages[source])
      if (language === 'zh') assert.notEqual(apiErrorText(source, translate, stateName), source)
    }
    const diagnostic = 'Unrecognized diagnostic /workspace/report.md {raw}'
    assert.equal(apiErrorText(diagnostic, translate, stateName), diagnostic)
  })
}
