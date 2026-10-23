import { test } from 'node:test'
import assert from 'node:assert/strict'
import { apiErrorText } from '../../src/apiErrorText.ts'
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
  'Unrecognized diagnostic /workspace/report.md {raw}',
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
