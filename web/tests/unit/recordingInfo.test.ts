import { test } from 'node:test'
import assert from 'node:assert/strict'
import { recordingReason, recordingReasons } from '../../src/runs/recordingInfo.ts'

test('recording status uses explicit causes and never classifies ordinary diagnostics', () => {
  for (const reason of recordingReasons.filter((r) => r !== 'unspecified')) {
    assert.equal(recordingReason({ status: 'incomplete', reason }), reason)
  }
  assert.equal(recordingReason({ status: 'expired' }), 'content_expired')
  assert.equal(recordingReason({ status: 'incomplete' }), 'unspecified')
  assert.equal(recordingReason({ status: 'truncated', reason: 'future_reason' }), 'unspecified')
  assert.equal(recordingReason({ reason: 'content_shortened' }), null)
  assert.equal(recordingReason({ status: 'diagnostic' }), null)
  assert.equal(recordingReason({ status: 'complete' }), null)
})
