import { test } from 'node:test'
import assert from 'node:assert/strict'
import { nodeIssues } from '../../src/dag/nodeIssues.ts'
import type { DagRunNode } from '../../src/api.ts'

test('current failed executions precede skipped descendants; old failures and pending retries are not counted', () => {
  const nodes = [
    { id: 'descendant', state: 'skipped' },
    { id: 'unconfirmed', state: 'unknown' },
    { id: 'root-failure', state: 'failed' },
    { id: 'recovered', state: 'succeeded', attempt_count: 2 },
    { id: 'retrying', state: 'failed', retry_at: '2099-01-01T00:00:00Z' },
    { id: 'cancelled', state: 'cancelled' },
  ] as DagRunNode[]
  const result = nodeIssues(nodes, 'warning')
  assert.deepEqual(
    result.issues.map((node) => node.id),
    ['root-failure', 'unconfirmed'],
  )
  assert.deepEqual(
    result.skipped.map((node) => node.id),
    ['descendant'],
  )
  assert.deepEqual(
    nodeIssues(nodes, 'unknown').issues.map((node) => node.id),
    ['unconfirmed', 'root-failure'],
  )
})

test('cancellation does not manufacture failed nodes while real failures remain inspectable', () => {
  const nodes = [
    { id: 'cancelled', state: 'cancelled' },
    { id: 'pending', state: 'waiting' },
  ] as DagRunNode[]
  assert.equal(nodeIssues(nodes, 'cancelled').issues.length, 0)
  assert.equal(nodeIssues(nodes, 'failed').issues.length, 0)
  nodes.push({ id: 'timed-out', state: 'timed_out' } as DagRunNode)
  assert.deepEqual(
    nodeIssues(nodes, 'cancelled').issues.map((node) => node.id),
    ['timed-out'],
  )
})
