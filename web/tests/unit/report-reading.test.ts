import { test } from 'node:test'
import assert from 'node:assert/strict'
import { reportIndex, referenceIdentity } from '../../src/moments/reportModel.ts'
import { ReportReadingStore } from '../../src/reports/readingState.ts'
import { defaultReportWidths } from '../../src/reports/layout.ts'
import { momentCollapseAnchor } from '../../src/moments/collapsePosition.ts'

test('report width preferences are shared within a workspace visit and isolated across workspaces', () => {
  const store = new ReportReadingStore()
  store.updateLayout('a', { navigation: 320, references: 420 })
  assert.deepEqual(store.getLayout('a'), { navigation: 320, references: 420 })
  assert.deepEqual(store.getLayout('b'), defaultReportWidths)
  assert.deepEqual(new ReportReadingStore().getLayout('a'), defaultReportWidths)
})
test('collapsing returns to the original card until the leading reading edge reaches another moment', () => {
  const positions = [
    { id: 'a', top: -500, bottom: 800 },
    { id: 'b', top: 800, bottom: 1100 },
  ]
  assert.deepEqual(momentCollapseAnchor('a', positions, 0, 120, 44), { id: 'a', top: 120 })
  assert.deepEqual(momentCollapseAnchor('a', positions, 0, -80, 44), { id: 'a', top: 0 })
  assert.deepEqual(
    momentCollapseAnchor(
      'a',
      [
        { id: 'a', top: -1300, bottom: 12 },
        { id: 'b', top: 12, bottom: 800 },
      ],
      0,
      120,
      44,
    ),
    { id: 'b', top: 12 },
  )
  assert.deepEqual(
    momentCollapseAnchor(
      'a',
      [
        { id: 'a', top: -1300, bottom: -500 },
        { id: 'b', top: -500, bottom: 800 },
      ],
      0,
      120,
      44,
    ),
    { id: 'b', top: -500 },
  )
})

const local = (href: string, cwd?: string) => referenceIdentity(href, true, cwd)
test('local identity folds only lexical aliases with a known execution directory', () => {
  assert.equal(local('file:///workspace/plots/one.png'), local('/workspace/plots/one.png'))
  assert.equal(local('./plots/one.png', '/workspace'), local('/workspace/plots/one.png'))
  assert.equal(local('plots//./one.png', '/workspace/'), local('/workspace/plots/one.png'))
  assert.notEqual(local('plots/one.png'), local('/workspace/plots/one.png'))
  assert.notEqual(local('/one/plot.png'), local('/two/plot.png'))
  assert.notEqual(local('/workspace/alias/../one.png'), local('/workspace/one.png'))
  assert.notEqual(local('/workspace/One.png'), local('/workspace/one.png'))
  assert.notEqual(local('/workspace/é.png'), local('/workspace/e\u0301.png'))
})
test('decode once, preserve literal plus, distinguish encoded filename delimiters from fragments', () => {
  assert.equal(local('/workspace/a%20b.png'), local('file:///workspace/a%20b.png#view'))
  assert.equal(local('/workspace/a%23b.png'), 'posix:/workspace/a#b.png')
  assert.equal(local('/workspace/a%3Fb.png'), 'posix:/workspace/a?b.png')
  assert.notEqual(local('/workspace/a%2523b.png'), local('/workspace/a%23b.png'))
  assert.notEqual(local('/workspace/a+b.png'), local('/workspace/a%20b.png'))
  assert.equal(local('/workspace/%E0%A4%A'), 'raw:/workspace/%E0%A4%A')
  assert.equal(local('/workspace/a%00b.png'), 'raw:/workspace/a%00b.png')
  assert.equal(local('file://host/share/a.png'), 'raw:file://host/share/a.png')
  assert.equal(local('//host/share/a.png'), 'raw://host/share/a.png')
})
test('Windows aliases require a Windows execution directory and retain filename case', () => {
  assert.equal(local('file:///C:/data/chart.png', 'C:\\data'), local('chart.png', 'C:\\data'))
  assert.equal(local('c:\\data\\chart.png', 'C:\\data'), local('C:/data/chart.png', 'C:\\data'))
  assert.notEqual(local('C:/data/Chart.png', 'C:\\data'), local('C:/data/chart.png', 'C:\\data'))
  assert.equal(local('C:\\data\\chart.png'), 'raw:C:\\data\\chart.png')
})
test('external identity never normalizes query, fragment or different origins', () => {
  assert.notEqual(
    referenceIdentity('https://one.test/a?x=1', false),
    referenceIdentity('https://one.test/a?x=2', false),
  )
  assert.notEqual(
    referenceIdentity('https://one.test/a#one', false),
    referenceIdentity('https://one.test/a#two', false),
  )
  assert.notEqual(
    referenceIdentity('https://one.test/a', false),
    referenceIdentity('https://two.test/a', false),
  )
})

function node(
  tagName: string,
  textContent: string,
  attrs: Record<string, string> = {},
  nested?: any,
): any {
  return {
    tagName,
    textContent,
    hasAttribute: (key: string) => key in attrs,
    getAttribute: (key: string) => attrs[key] ?? null,
    querySelector: () => nested ?? null,
  }
}
function root(headings: any[], links: any[]): ParentNode {
  return {
    querySelectorAll: (selector: string) => (selector.startsWith('h1,') ? headings : links),
  } as unknown as ParentNode
}
test('file-first and image-first references merge without rewriting the original preview href', () => {
  const file = node('BUTTON', 'Report file', {
    'data-result-file-href': 'file:///workspace/chart.png',
  })
  const image = node('SPAN', 'Loading… Retry', {
    'data-result-image-href': './chart.png',
    'data-result-image-label': 'Quarterly chart',
  })
  for (const ordered of [
    [file, image],
    [image, file],
  ]) {
    const { links } = reportIndex(root([], ordered), '/workspace')
    assert.equal(links.length, 1)
    assert.equal(links[0].href, ordered[0].getAttribute('data-result-file-href') ?? './chart.png')
    assert.equal(links[0].image, true)
    assert.equal(links[0].label, 'Quarterly chart')
    assert.equal(links[0].index, 0)
  }
})
test('linked images, external sources and same-named distinct files retain correct order and identity', () => {
  const image = node('SPAN', 'Image unavailable', {
    'data-result-image-href': '/workspace/chart.png',
    'data-result-image-label': 'Chart',
  })
  const { links } = reportIndex(
    root(
      [],
      [
        node(
          'BUTTON',
          'Loading…',
          { 'data-result-file-href': 'file:///workspace/chart.png' },
          image,
        ),
        image,
        node('A', 'Original source', { href: 'https://example.test/source?id=1' }, image),
        node('A', 'Different source', { href: 'https://example.test/source?id=2' }),
        node('BUTTON', 'Chart', { 'data-result-file-href': '/other/chart.png' }),
        node('A', 'Unsafe', { href: 'javascript:alert(1)' }),
      ],
    ),
    '/workspace',
  )
  assert.deepEqual(
    links.map(({ label, index, image }) => ({ label, index, image })),
    [
      { label: 'Chart', index: 0, image: true },
      { label: 'Chart', index: 2, image: false },
      { label: 'Different source', index: 3, image: false },
      { label: 'Chart', index: 4, image: false },
    ],
  )
})
test('heading keys survive inserted sections and distinguish repeated headings', () => {
  const headings = [
    node('H1', 'Report'),
    node('H2', 'Finding'),
    node('H3', 'Finding'),
    node('H2', 'Finding'),
  ]
  const before = reportIndex(root(headings, [])).headings
  const after = reportIndex(root([node('H1', 'New introduction'), ...headings], [])).headings
  assert.deepEqual(
    after.slice(1).map((h) => h.key),
    before.map((h) => h.key),
  )
  assert.equal(new Set(before.map((h) => h.key)).size, 4)
  assert.deepEqual(
    before.map((h) => h.index),
    [0, 1, 2, 3],
  )
})
test('reading state is stable and isolated by workspace, parent Run, output and actual execution', () => {
  const store = new ReportReadingStore()
  const key = 'workspace-a:run-a:output-a:execution-a'
  const initial = store.get(key)
  assert.equal(store.get(key), initial)
  let changes = 0
  const unsubscribe = store.subscribe(() => changes++)
  store.update(key, {
    referencesExpanded: true,
    outlineScroll: 80,
    anchor: { heading: '2:Finding:0', offset: 64 },
  })
  assert.notEqual(store.get(key), initial)
  assert.equal(initial.referencesExpanded, false)
  for (const other of [
    'workspace-b:run-a:output-a:execution-a',
    'workspace-a:run-b:output-a:execution-a',
    'workspace-a:run-a:output-b:execution-a',
    'workspace-a:run-a:output-a:execution-b',
  ]) {
    assert.equal(store.get(other).referencesExpanded, false)
    assert.equal(store.get(other).anchor, null)
  }
  assert.equal(store.get(key).outlineScroll, 80)
  store.update(key, { referencesExpanded: true })
  assert.equal(changes, 1)
  unsubscribe()
  store.update(key, { referencesExpanded: false })
  assert.equal(changes, 1)
  assert.equal(new ReportReadingStore().get(key).anchor, null)
})
