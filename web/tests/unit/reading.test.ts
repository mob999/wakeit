import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  freshReading,
  visibleSummary,
  inputSummary,
  previewLines,
  isExpanded,
  followingTail,
} from '../../src/runs/readingModel.ts'
test('summaries retain real text and command/path inputs without changing the original payload', () => {
  assert.equal(visibleSummary('# **Report**\n\n[More](./report.md)'), 'Report')
  assert.equal(visibleSummary(' \n\n 中文 **内容**'), '中文 内容')
  assert.equal(inputSummary({ path: '/workspace/report.md', text: 'body' }), '/workspace/report.md')
  assert.equal(inputSummary({ command: 'echo\n hello' }), 'echo hello')
  assert.equal(visibleSummary(null), '')
})
test('only the latest message automatically expands and explicit folding always wins', () => {
  const state = freshReading()
  assert.equal(isExpanded(state, 'old', 'latest'), false)
  assert.equal(isExpanded(state, 'latest', 'latest'), true)
  state.expanded.latest = false
  assert.equal(isExpanded(state, 'latest', 'latest'), false)
  state.autoExpand = false
  assert.equal(isExpanded(state, 'new', 'new'), false)
  state.autoExpand = true
  state.autoId = 'old'
  state.follow = false
  assert.equal(isExpanded(state, 'old', 'new'), true)
  assert.equal(isExpanded(state, 'new', 'new'), false)
})
test('preview is twelve lines, full output remains exact, following threshold includes 48 pixels', () => {
  const text = Array.from({ length: 20 }, (_, i) => 'line ' + i).join('\n')
  assert.equal(previewLines(text, false).text.split('\n').length, 12)
  assert.equal(previewLines(text, true).text, text)
  assert.equal(previewLines(text, false).truncated, true)
  assert.equal(followingTail(48), true)
  assert.equal(followingTail(49), false)
})
