import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
const resources = ['zh', 'en'].map((lang) =>
  JSON.parse(readFileSync(new URL(`../../src/locales/${lang}.json`, import.meta.url), 'utf8')),
)
test('action names and completion interpolation have matching zh/en contracts', () => {
  const keys = resources.map((resource) =>
    Object.keys(resource)
      .filter((key) => key.startsWith('actions.'))
      .sort(),
  )
  assert.deepEqual(keys[0], keys[1])
  for (const key of keys[0]) {
    for (const resource of resources) assert.ok(resource[key].trim(), key)
    assert.deepEqual(
      resources[0][key].match(/\{\{[^}]+\}\}/g),
      resources[1][key].match(/\{\{[^}]+\}\}/g),
      key,
    )
  }
})
