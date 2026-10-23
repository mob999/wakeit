import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import i18next from 'i18next'
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

for (const prefix of ['actions.', 'retry.error.', 'agentPrompt.']) {
  test(prefix + ' translations have matching keys, text and interpolation parameters', () => {
    const keys = Object.values(resources).map(({ translation }) =>
      Object.keys(translation)
        .filter((key) => key.startsWith(prefix))
        .sort(),
    )
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
}

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

for (const language of ['zh', 'en']) {
  test(
    language + ': real i18next preserves diagnostic identifiers during interpolation',
    async () => {
      const i18n = i18next.createInstance()
      await i18n.init({
        resources,
        lng: language,
        fallbackLng: false,
        keySeparator: false,
        interpolation: { escapeValue: false },
      })
      const node = '/workspace/中文 report.md {{literal}}'
      const text = i18n.t('retry.error.resultUnavailable', { node })
      assert.ok(text.includes(node))
      assert.notEqual(text, 'retry.error.resultUnavailable')
      for (const key of literalDiagnostics) {
        assert.ok(i18n.exists(key), key)
        assert.ok(i18n.t(key).trim())
        if (language === 'zh') assert.notEqual(i18n.t(key), key)
      }
    },
  )
}
