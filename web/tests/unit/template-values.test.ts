import assert from 'node:assert/strict'
import test from 'node:test'
import { parseOverrides, parseJsonDocument } from '../../src/templates/document.ts'
import { promptReferences, promptBlank, promptText } from '../../src/dag/prompt.ts'
test('input documents retain false, zero, empty, null and omission as separate values', () => {
  const source =
    "values: {flag: false, count: 0, text: '', list: [], object: {}, nullable: null}\nunset: [optional]"
  assert.deepEqual(parseOverrides(source), {
    values: { flag: false, count: 0, text: '', list: [], object: {}, nullable: null },
    unset: ['optional'],
  })
  for (const source of [
    'values: {a: 1, a: 2}',
    '{"values":{"a":1,"a":2}}',
    'values: {1: value}',
    'values: {x: !custom text}',
    'values: {x: .inf}',
    'values: {x: 9007199254740993}',
    '---\nvalues: {}\n---\nvalues: {}',
    'values: &a {x: 1}\nother: *a',
  ])
    assert.throws(() => parseJsonDocument(source), source)
  assert.throws(() => parseOverrides('{count: 1}'))
  assert.throws(() => parseOverrides('values: []'))
})
test('typed prompt references ignore reference-looking literal content and preserve repeated result parts', () => {
  const prompt = {
    parts: [
      { text: '{{ nodes.fake.result }}' },
      { result: 'source' },
      { text: '\n' },
      { result: 'source' },
    ],
  }
  assert.deepEqual(promptReferences(prompt), ['source', 'source'])
  assert.deepEqual(promptReferences('{{ nodes.legacy.result }}'), ['legacy'])
  assert.equal(
    promptText(prompt),
    '{{ nodes.fake.result }}{{ nodes.source.result }}\n{{ nodes.source.result }}',
  )
  assert.equal(promptBlank({ parts: [{ text: '  ' }] }), true)
  assert.equal(promptBlank({ parts: [{ result: 'source' }] }), false)
})
