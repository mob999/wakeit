import { test } from 'node:test'
import assert from 'node:assert/strict'
import { duration, retriedNodes, linkName } from '../../src/moments/reportModel.ts'
test('elapsed time is factual, includes graph waiting, and never invents a start', () => {
  assert.equal(duration(null, '2026-10-21T01:00:00Z'), null)
  assert.equal(duration('invalid', '2026-10-21T01:00:00Z'), null)
  assert.equal(duration('2026-10-21T01:01:00Z', '2026-10-21T01:00:00Z'), null)
  assert.equal(duration('2026-10-21T01:00:00Z', '2026-10-21T01:06:42Z'), '6m 42s')
  assert.equal(duration('2026-10-21T01:00:00Z', '2026-10-21T03:05:00Z'), '2h 5m')
})
test('retried nodes count actual additional attempts, not every failed output', () => {
  assert.equal(retriedNodes(), 0)
  assert.equal(
    retriedNodes({
      nodes: [{ attempt_count: 1, state: 'failed' }, { attempt_count: 2 }, { attempt_count: 3 }],
    } as any),
    2,
  )
})
test('link labels retain authored titles and safely shorten local/remote targets', () => {
  assert.equal(linkName('/workspace/notes%20one.md', ''), 'notes one.md')
  assert.equal(linkName('C:\\data\\report.md', ''), 'report.md')
  assert.equal(linkName('https://example.com/report', 'Reference'), 'Reference')
  assert.equal(linkName('/workspace/%E0%A4%A', ''), '/workspace/%E0%A4%A')
})
