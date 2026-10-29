import { test } from 'node:test'
import assert from 'node:assert/strict'
import { apiErrorText } from '../../src/api/errorText.ts'
import { retryDiagnostics, literalDiagnostics } from './diagnostic-cases.ts'

// Test routing independently of either catalog or an imitation of i18next.
for (const [source, key, values] of retryDiagnostics) {
  test('diagnostic routing: ' + source, () => {
    const calls: unknown[] = []
    const result = apiErrorText(
      source,
      (key, values) => {
        calls.push({ key, values })
        return 'translated'
      },
      (state) => 'state:' + state,
    )
    assert.deepEqual(calls, [{ key, values }])
    assert.equal(result, 'translated')
  })
}

for (const source of [
  ...literalDiagnostics,
  'Unrecognized diagnostic /workspace/中文 report.md {{literal}}',
  'Agent output: Upstream result is empty: collect',
]) {
  test('unmatched diagnostics retain the complete translation key: ' + source, () => {
    assert.equal(
      apiErrorText(
        source,
        (key) => key,
        (state) => state,
      ),
      source,
    )
  })
}

test('routed diagnostic parameters preserve Unicode and literal braces', () => {
  const node = '/workspace/中文 report.md {{literal}}'
  const calls: unknown[] = []
  apiErrorText(
    'Upstream result unavailable: ' + node,
    (key, values) => {
      calls.push({ key, values })
      return 'translated'
    },
    (state) => state,
  )
  assert.deepEqual(calls, [{ key: 'retry.error.resultUnavailable', values: { node } }])
})
