import { test } from 'node:test'
import assert from 'node:assert/strict'
import { recordingReason } from '../../src/runs/recordingInfo.ts'

// Cases describe the protocol contract, not the implementation's allowlist.
for (const reason of [
  'content_shortened',
  'event_limit',
  'event_unsupported',
  'event_parse_failed',
  'event_too_large',
  'content_limit',
  'content_expired',
]) {
  test('recording cause ' + reason + ' survives both incomplete statuses', () => {
    for (const status of ['incomplete', 'truncated'])
      assert.equal(recordingReason({ status, reason }), reason, status)
  })
}
test('expired content takes precedence over a stale recording cause', () => {
  assert.equal(recordingReason({ status: 'expired', reason: 'event_limit' }), 'content_expired')
})
test('missing and future causes remain explicitly unspecified', () => {
  assert.equal(recordingReason({ status: 'incomplete' }), 'unspecified')
  assert.equal(recordingReason({ status: 'truncated', reason: 'future_reason' }), 'unspecified')
})
test('ordinary diagnostics and complete records never imply lost activity', () => {
  for (const status of [undefined, 'diagnostic', 'complete'])
    assert.equal(recordingReason({ status, reason: 'content_shortened' }), null)
})
