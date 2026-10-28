import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  freshReading,
  visibleSummary,
  inputSummary,
  previewLines,
  isExpanded,
  runTab,
} from '../../src/runs/readingModel.ts'

test('message summaries use the first visible paragraph', () => {
  assert.equal(visibleSummary('# **Report**\n\n[More](./report.md)'), 'Report')
  assert.equal(visibleSummary(' \n\n 中文 **内容**'), '中文 内容')
  assert.equal(visibleSummary(null), '')
})
test('tool summaries prefer path or command without changing the payload', () => {
  const input = { path: '/workspace/report.md', text: 'body' }
  assert.equal(inputSummary(input), '/workspace/report.md')
  assert.deepEqual(input, { path: '/workspace/report.md', text: 'body' })
  assert.equal(inputSummary({ command: 'echo\n hello' }), 'echo hello')
})
test('latest expansion is opt-in after returning to the latest message', () => {
  const state = { ...freshReading(), autoExpand: true }
  assert.equal(isExpanded(state, 'old', 'latest'), false)
  assert.equal(isExpanded(state, 'latest', 'latest'), true)
})
test('explicit row choices survive automatic expansion and collapse-all', () => {
  const state = freshReading()
  state.expanded.latest = false
  state.expanded.old = true
  assert.equal(isExpanded(state, 'latest', 'latest'), false)
  state.autoExpand = false
  assert.equal(isExpanded(state, 'old', 'latest'), true)
  assert.equal(isExpanded(state, 'new', 'new'), false)
})
test('reading history keeps its explicit latest row when a newer message arrives', () => {
  const state = { ...freshReading(), autoExpand: true }
  state.autoId = 'old'
  state.follow = false
  assert.equal(isExpanded(state, 'old', 'new'), true)
  assert.equal(isExpanded(state, 'new', 'new'), false)
})
test('payload preview truncates only beyond twelve lines and full mode preserves bytes', () => {
  const lines = Array.from({ length: 13 }, (_, i) => 'line ' + i)
  const boundary = lines.slice(0, 12).join('\n')
  assert.deepEqual(previewLines(boundary, false), { text: boundary, truncated: false })
  const text = lines.join('\n')
  assert.deepEqual(previewLines(text, false), { text: boundary, truncated: true })
  assert.equal(previewLines(text, true).text, text)
})
test('run tabs default to Trace while waiting and Output when available', () => {
  const state = freshReading()
  assert.equal(runTab(state, false), 'trace')
  assert.equal(runTab(state, true), 'output')
  state.follow = false
  assert.equal(runTab(state, true), 'trace')
})
test('explicit tabs survive result arrival, follow changes and unavailable output', () => {
  for (const viewTab of ['output', 'trace', 'details'] as const)
    for (const follow of [true, false])
      for (const available of [true, false])
        assert.equal(runTab({ ...freshReading(), viewTab, follow }, available), viewTab)
})

test('first visit keeps all Trace entries collapsed, including the latest and new messages', () => {
  const state = freshReading()
  for (const latest of ['first', 'newest'])
    for (const id of ['tool', 'first', 'newest']) assert.equal(isExpanded(state, id, latest), false)
  state.expanded.first = true
  assert.equal(isExpanded(state, 'first', 'newest'), true)
  assert.equal(isExpanded(state, 'newest', 'newest'), false)
})
