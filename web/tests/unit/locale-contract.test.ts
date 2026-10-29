import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { retryDiagnostics, literalDiagnostics } from './diagnostic-cases.ts'

const resources = Object.fromEntries(
  ['zh', 'en'].map((lang) => [
    lang,
    {
      translation: JSON.parse(
        readFileSync(new URL('../../src/locales/' + lang + '.json', import.meta.url), 'utf8'),
      ),
    },
  ]),
)

test('catalogs have matching keys, nonempty text and interpolation parameters', () => {
  const keys = Object.values(resources).map(({ translation }) => Object.keys(translation).sort())
  assert.ok(keys[0].length > 0)
  assert.deepEqual(keys[0], keys[1])
  for (const key of keys[0]) {
    const values = Object.values(resources).map(({ translation }) => translation[key] as string)
    assert.ok(
      values.every((value) => value.trim()),
      key,
    )
    assert.deepEqual(
      values[0].match(/\{\{[^}]+\}\}/g)?.sort(),
      values[1].match(/\{\{[^}]+\}\}/g)?.sort(),
      key,
    )
  }
})

test('all routed diagnostics have their required catalog keys and named parameters', () => {
  for (const { translation } of Object.values(resources)) {
    for (const [, key, values] of retryDiagnostics) {
      assert.ok(Object.hasOwn(translation, key), key)
      const fields = [...(translation[key] as string).matchAll(/\{\{(\w+)\}\}/g)].map(
        (match) => match[1],
      )
      assert.deepEqual(fields.sort(), Object.keys(values).sort(), key)
    }
  }
})

test('literal diagnostics are translated in both catalogs', () => {
  for (const [language, { translation }] of Object.entries(resources)) {
    for (const key of literalDiagnostics) {
      assert.ok(Object.hasOwn(translation, key), key)
      assert.ok(translation[key].trim(), key)
      if (language === 'zh') assert.notEqual(translation[key], key)
    }
  }
})
