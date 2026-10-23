import { test, expect, type Page } from '@playwright/test'
import { workspacePage, mockWorkspace, apiPath } from './workspace'
const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`
export async function momentsFixture(
  page: Page,
  options: { count?: number; daySize?: number; state?: string; fail?: boolean } = {},
) {
  const calls: string[] = []
  const count = options.count ?? 4,
    size = options.daySize ?? 24
  const config = {
    version: 1,
    id: 'report',
    name: 'Synthetic report',
    runner: 'Review device',
    agent: 'pi',
    cwd: '/workspace',
    prompt: 'Synthetic instruction',
    parameters: {},
    schedule: null,
    enabled: true,
    execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
  }
  const runs: any[] = Array.from({ length: count }, (_, i) => {
    const day = new Date(Date.UTC(2026, 9, 21 - Math.floor(i / size))).toISOString().slice(0, 10)
    return {
      id: id(i + 1),
      config: { ...config, name: `Research report ${i + 1} · 多语言任务` },
      task_id: 'report',
      state: options.state ?? (i === 2 ? 'failed' : 'succeeded'),
      reason: i === 2 ? 'Agent exited before producing a report.' : null,
      source: 'manual',
      created_at: `${day}T09:00:00Z`,
      finished_at: `${day}T09:${String(59 - (i % size)).padStart(2, '0')}:00Z`,
      result: `# Research ${i + 1}\n\n[Local report](/workspace/report.md)\n\n${Array.from({ length: 15 }, (_, n) => `${n === 0 ? '## Findings\n\n' : n === 10 ? '## References\n\n' : ''}Paragraph ${n + 1}: synthetic content for continuous reading, with **important findings** and a [source](https://example.com).`).join('\n\n')}`,
      execution_snapshot: { trace_supported: false, cwd: '/workspace' },
    }
  })
  await page.route('**/api/v1/**', async (route) => {
    if (await mockWorkspace(route)) return
    const url = new URL(route.request().url()),
      path = apiPath(url.pathname)
    calls.push(path + url.search)
    if (path === '/session')
      return route.fulfill({ json: { id: 'reviewer', username: 'reviewer', role: 'admin' } })
    if (path === '/meta') return route.fulfill({ json: { version: '0.3.1' } })
    if (path === '/settings') return route.fulfill({ json: { timezone: 'UTC' } })
    if (path === '/moments/days') {
      const dates = [...new Set(runs.map((r) => r.finished_at.slice(0, 10)))].filter(
        (d) => !url.searchParams.get('before') || d < url.searchParams.get('before')!,
      )
      return route.fulfill({
        json: {
          timezone: 'UTC',
          today: '2026-10-21',
          total_runs: count,
          days: dates.slice(0, 14).map((date) => ({
            date,
            count: runs.filter((r) => r.finished_at.startsWith(date)).length,
          })),
          next_before: dates.length > 14 ? dates[13] : null,
        },
      })
    }
    if (path === '/moments') {
      const date = url.searchParams.get('date'),
        offset = Number(url.searchParams.get('cursor') ?? 0)
      const day = runs.filter((r) => r.finished_at.startsWith(date)),
        batch = day.slice(offset, offset + 20)
      if (options.fail)
        return route.fulfill({ status: 503, json: { error: 'Synthetic connection failure' } })
      return route.fulfill({
        json: {
          timezone: 'UTC',
          date,
          items: batch.map((r) => ({
            ...r,
            name: r.config.name,
            runner: config.runner,
            agent: 'Pi',
            preview: typeof r.result === 'string' ? r.result.slice(0, 2048) : null,
            format: r.config.dag ? 'dag' : 'markdown',
            node_count: r.config.dag?.nodes.length,
            has_more: true,
            truncated: false,
            continuation_available: false,
          })),
          next_cursor: offset + batch.length < day.length ? String(offset + batch.length) : null,
        },
      })
    }
    if (path === '/runs')
      return route.fulfill({
        json: { items: runs.slice(0, 20), total: runs.length, page: 1, page_size: 20 },
      })
    if (path.endsWith('/files/preview') && route.request().postDataJSON().href.endsWith('.png'))
      return route.fulfill({
        json: {
          path: '/workspace/render.png',
          name: 'render.png',
          format: 'image',
          mime: 'image/png',
          size: 68,
          truncated: false,
          content:
            'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAusB9Wl6XzQAAAAASUVORK5CYII=',
        },
      })
    if (path.endsWith('/files/preview'))
      return route.fulfill({
        json: {
          path: '/workspace/report.md',
          name: 'report.md',
          format: 'markdown',
          content: '# Synthetic preview\n\nPreview stays open.',
          runner_id: 'device',
          mime: 'text/markdown',
          size: 50,
          modified_at: null,
          truncated: false,
        },
      })
    if (path.endsWith('/stream'))
      return route.fulfill({ contentType: 'text/event-stream', body: ':fixture\n\n' })
    const run = runs.find((r) => path === `/runs/${r.id}`)
    if (run) return route.fulfill({ json: run })
    return route.fulfill({ json: [] })
  })
  return { calls, runs }
}
test('timeline cards expose real outline and links, short reports shrink and code links stay literal', async ({
  page,
}) => {
  const { runs } = await momentsFixture(page)
  runs[1].result = 'A short report.'
  runs[2].result =
    '# Safety\n\n```markdown\n[Do not index](/workspace/code.md)\n```\n\n[Bad](javascript:alert(1))'
  await page.goto(workspacePage('/'))
  const first = page.locator(`[data-moment-id="${id(1)}"]`)
  await expect(first.getByRole('navigation', { name: '目录', exact: true })).toBeVisible()
  await expect(
    first.locator('.moment-link-list').getByRole('button', { name: 'Local report', exact: true }),
  ).toBeVisible()
  const short = page.locator(`[data-moment-id="${id(2)}"]`)
  await expect(short.locator('.moment-report')).toContainText('A short report.')
  await expect(short.locator('.moment-context')).toHaveCount(0)
  expect(
    await short.locator('.moment-report').evaluate((el) => el.getBoundingClientRect().height),
  ).toBeLessThan(40)
  const safety = page.locator(`[data-moment-id="${id(3)}"]`)
  await expect(safety.locator('.moment-link-list')).toHaveCount(0)
  await first
    .getByRole('navigation', { name: '目录', exact: true })
    .getByRole('button', { name: 'References', exact: true })
    .click()
  await expect(first.getByRole('button', { name: '收起', exact: true })).toBeVisible()
  await expect
    .poll(() => page.evaluate(() => document.activeElement?.textContent))
    .toBe('References')
})
test('context file previews survive manual refresh, and Viewer copies paths without file IO', async ({
  page,
  context,
}) => {
  const { calls } = await momentsFixture(page, { count: 1 })
  await page.goto(workspacePage('/'))
  await page
    .locator('.moment-link-list')
    .getByRole('button', { name: 'Local report', exact: true })
    .click()
  await expect(page.getByRole('dialog')).toContainText('Synthetic preview')
  await page.locator('button[aria-label="刷新动态"]').dispatchEvent('click')
  await expect(page.getByRole('dialog')).toBeVisible()
  expect(calls.filter((p) => p.endsWith('/files/preview'))).toHaveLength(1)
  await page.getByRole('dialog').getByRole('button', { name: '关闭', exact: true }).click()
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  await page.route('**/api/v1/session', (route) =>
    route.fulfill({ json: { id: 'reviewer', username: 'reviewer', role: 'viewer' } }),
  )
  await page.route('**/api/v1/workspaces/11111111-2222-4333-8444-555555555555', (route) =>
    route.fulfill({
      json: {
        id: '11111111-2222-4333-8444-555555555555',
        name: 'Fixture',
        role: 'viewer',
        is_demo: false,
      },
    }),
  )
  await page.reload()
  await page
    .locator('.moment-link-list')
    .getByRole('button', { name: 'Local report', exact: true })
    .click()
  await expect
    .poll(() => page.evaluate(() => navigator.clipboard.readText()))
    .toBe('/workspace/report.md')
  expect(calls.filter((p) => p.endsWith('/files/preview'))).toHaveLength(1)
  await expect(page.getByRole('dialog')).toHaveCount(0)
})
test('DAG output switching selects the actual report and preserves empty-preview loading semantics', async ({
  page,
}) => {
  const { runs, calls } = await momentsFixture(page, { count: 1 })
  const node = (name: string) => ({
    id: name,
    name,
    pre: [],
    state: 'succeeded',
    runner: 'device',
    agent: 'pi',
    prompt: 'read',
    cwd: '/workspace',
    parameters: {},
    attempt_count: name === 'Second output' ? 2 : 1,
    continuation_available: false,
  })
  runs[0].config.dag = { nodes: [node('First output'), node('Second output')] }
  runs[0].nodes = runs[0].config.dag.nodes
  runs[0].started_at = '2026-10-21T09:57:00Z'
  runs[0].result = {
    type: 'dag',
    outputs: [
      {
        node_id: 'First output',
        name: 'First output',
        state: 'succeeded',
        result: '# First report',
      },
      {
        node_id: 'Second output',
        name: 'Second output',
        state: 'warning',
        result: '# Second report\n\n[Second file](/workspace/second.md)',
      },
    ],
  }
  await page.route(`**/runs/${id(1)}/nodes/Second%20output`, (route) => {
    calls.push('/actual-node')
    return route.fulfill({
      json: { node: node('Second output'), execution: { ...runs[0], id: id(9) } },
    })
  })
  await page.goto(workspacePage('/'))
  const card = page.locator('.moment-card')
  await expect(card.getByRole('heading', { name: 'First report', exact: true })).toBeVisible()
  await card
    .locator('.moment-output-list')
    .getByRole('button', { name: /Second output/ })
    .click()
  await expect(card.getByRole('heading', { name: 'Second report', exact: true })).toBeVisible()
  await expect(card.locator('.moment-retries')).toContainText('1 个节点')
  await card
    .locator('.moment-link-list')
    .getByRole('button', { name: 'Second file', exact: true })
    .click()
  await expect(page.getByRole('dialog')).toBeVisible()
  expect(calls).toContain('/actual-node')
  expect(calls.some((p) => p.endsWith(`/runs/${id(9)}/files/preview`))).toBeTruthy()
  await expect(page.locator('.moment-entry')).toHaveCount(1)
})
test('only nearby cards hydrate full reports; failures preserve an already loaded body', async ({
  page,
}) => {
  const { calls } = await momentsFixture(page, { count: 60, daySize: 20 })
  await page.goto(workspacePage('/'))
  await expect(page.locator('.moment-entry')).toHaveCount(60)
  await expect(page.locator('.moment-context').first()).toBeVisible()
  await page.waitForTimeout(500)
  expect(new Set(calls.filter((p) => /^\/runs\/[^/]+$/.test(p))).size).toBeLessThan(10)
  expect(await page.locator('.moment-report').count()).toBeLessThan(10)
  await page.route(`**/runs/${id(1)}`, (route) =>
    route.fulfill({ status: 503, json: { error: 'Synthetic refresh failure' } }),
  )
  await page.getByRole('button', { name: '刷新动态', exact: true }).click()
  await expect(page.getByRole('alert')).toContainText('Synthetic refresh failure')
  await expect(page.locator(`[data-moment-id="${id(1)}"] .moment-report`)).toContainText(
    'Research 1',
  )
  await expect(
    page.locator(`[data-moment-id="${id(1)}"]`).getByRole('button', { name: '重试', exact: true }),
  ).toBeVisible()
})
test('synthetic responsive preview preserves card metrics and timeline in both languages and themes', async ({
  page,
}, testInfo) => {
  test.setTimeout(90000)
  await momentsFixture(page)
  for (const language of ['zh', 'en'])
    for (const theme of ['light', 'dark']) {
      await page.addInitScript(
        ({ language, theme }) => {
          localStorage.setItem('wakeit-language', language)
          localStorage.setItem('wakeit-theme', theme)
        },
        { language, theme },
      )
      await page.goto(workspacePage('/'))
      await expect(page.locator('.moment-card').first()).toBeVisible()
      for (const width of [1440, 2200, 1024, 390]) {
        await page.setViewportSize({ width, height: 900 })
        await page.waitForTimeout(100)
        expect(
          await page.evaluate(() => document.documentElement.scrollWidth > innerWidth),
        ).toBeFalsy()
        expect(
          await page
            .locator('.moment-card > .card-header')
            .first()
            .evaluate((el) => getComputedStyle(el).minHeight),
        ).toBe('44px')
        await page.screenshot({
          path: testInfo.outputPath(`moments-${language}-${theme}-${width}.png`),
        })
      }
    }
})

test('more entries preserve date groups and manual refresh does not start polling', async ({
  page,
}) => {
  const { calls } = await momentsFixture(page, { count: 24, daySize: 22 })
  await page.goto(workspacePage('/'))
  await expect(page.locator('.moment-entry')).toHaveCount(22)
  const day = page.getByRole('region', { name: '2026-10-21', exact: true })
  await day.getByRole('button', { name: '加载更多', exact: true }).click()
  await expect(page.locator('.moment-entry')).toHaveCount(24)
  await page.waitForTimeout(500)
  const initial = calls.filter((p) => p.startsWith('/moments')).length
  await page.evaluate(() => window.dispatchEvent(new Event('focus')))
  await page.waitForTimeout(2200)
  expect(calls.filter((p) => p.startsWith('/moments')).length).toBe(initial)
  await page.getByRole('button', { name: '刷新动态', exact: true }).click()
  await expect
    .poll(() => calls.filter((p) => p.startsWith('/moments')).length)
    .toBeGreaterThan(initial)
})
test('empty state, request retry, JSON and truncated reports remain distinct', async ({ page }) => {
  const options = { count: 0, fail: false }
  await momentsFixture(page, options)
  await page.goto(workspacePage('/'))
  await expect(page.getByText('暂无执行结果', { exact: true })).toBeVisible()
  await page.unroute('**/api/v1/**')
  const failure = { count: 1, fail: true }
  const { runs } = await momentsFixture(page, failure)
  runs[0].result = { truncated: true, preview: '# Retained text\n\nPartial content' }
  await page.reload()
  await expect(page.getByRole('alert')).toContainText('Synthetic connection failure')
  failure.fail = false
  await page.getByRole('button', { name: '重试', exact: true }).click()
  await expect(page.locator('.moment-notice')).toContainText('结果已截断')
  await expect(page.locator('.moment-report')).toContainText('Retained text')
  runs[0].result = { ok: true, count: 3 }
  await page.getByRole('button', { name: '刷新动态', exact: true }).click()
  await expect(page.locator('.moment-report code')).toContainText('\"ok\": true')
})
test('image references open previews, and open previews pin their source card', async ({
  page,
  context,
}) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  const { runs, calls } = await momentsFixture(page, { count: 40, daySize: 20 })
  runs[0].result =
    '# Images\n\n![Rendered picture](/workspace/render.png)\n\n[Report](/workspace/report.md)'
  await page.goto(workspacePage('/'))
  const card = page.locator(`[data-moment-id="${id(1)}"]`)
  await expect(card.getByRole('img', { name: 'Rendered picture', exact: true })).toBeVisible()
  await card
    .locator('.moment-link-list')
    .getByRole('button', { name: 'Rendered picture', exact: true })
    .click()
  await expect(page.getByRole('dialog')).toBeVisible()
  await expect(page.getByRole('dialog').getByRole('img')).toBeVisible()
  expect(calls.filter((p) => p.endsWith('/files/preview'))).toHaveLength(2)
  await page.locator('button[aria-label="刷新动态"]').dispatchEvent('click')
  await expect(page.getByRole('dialog')).toBeVisible()
  await page.getByRole('dialog').getByRole('button', { name: '关闭', exact: true }).click()
  await card
    .locator('.moment-link-list')
    .getByRole('button', { name: 'Report', exact: true })
    .click()
  await expect(page.getByRole('dialog')).toBeVisible()
  await page.locator('.moments-page').evaluate((el) => {
    el.scrollTop = el.scrollHeight - 800
  })
  await page.waitForTimeout(400)
  await expect(page.getByRole('dialog')).toBeVisible()
  await page.getByRole('dialog').getByRole('button', { name: '关闭', exact: true }).click()
  await expect(card.locator('.moment-placeholder')).toHaveCount(1)
})

test('a long outline keeps readable rows and scrolls to every heading after expansion', async ({
  page,
}) => {
  const { runs } = await momentsFixture(page, { count: 1 })
  runs[0].result = Array.from(
    { length: 68 },
    (_, n) => `## Section ${n + 1}\n\nSynthetic section ${n + 1}.`,
  ).join('\n\n')
  await page.goto(workspacePage('/'))
  const card = page.locator('.moment-card')
  const outline = card.getByRole('navigation', { name: '目录', exact: true })
  const more = outline.locator('..').getByRole('button', { name: '另外 65 项', exact: true })
  await more.click()
  await expect(outline.getByRole('button')).toHaveCount(68)
  await expect(more).toHaveAttribute('aria-expanded', 'true')
  expect(
    await outline
      .getByRole('button')
      .evaluateAll((rows) => Math.min(...rows.map((row) => row.getBoundingClientRect().height))),
  ).toBeGreaterThanOrEqual(20)
  expect(await outline.evaluate((el) => el.scrollHeight > el.clientHeight)).toBe(true)
  await outline.getByRole('button', { name: 'Section 68', exact: true }).click()
  await expect
    .poll(() => page.evaluate(() => document.activeElement?.textContent))
    .toBe('Section 68')
  await outline.locator('..').getByRole('button', { name: '收起', exact: true }).click()
  await expect(outline.getByRole('button')).toHaveCount(3)
})

test('device and Agent stay together on the right of the card header', async ({ page }) => {
  await momentsFixture(page, { count: 1 })
  await page.goto(workspacePage('/'))
  const header = page.locator('.moment-card > .card-header')
  await expect(header.locator('.moment-identity')).toContainText('Review device')
  await expect(header.locator('.moment-identity')).toContainText('Pi')
  const bounds = await header.boundingBox(),
    identity = await header.locator('.moment-identity').boundingBox(),
    title = await header.locator('h3').boundingBox()
  expect(identity!.x).toBeGreaterThan(title!.x + title!.width)
  expect(identity!.y + identity!.height).toBeLessThanOrEqual(bounds!.y + bounds!.height)
  await expect(page.locator('.moment-meta')).toHaveCount(0)
})

test('references fill the reading height, adapt to expansion and narrow columns, without trailing arrows', async ({
  page,
}, testInfo) => {
  const { runs } = await momentsFixture(page, { count: 1 })
  runs[0].result = Array.from(
    { length: 30 },
    (_, n) =>
      `A synthetic finding with supporting [Reference ${n + 1}](https://example.test/source/${n + 1}).`,
  ).join('\n\n')
  await page.goto(workspacePage('/'))
  const card = page.locator('.moment-card')
  const list = card.locator('.moment-link-list')
  const references = list.locator('a:visible')
  const section = list.locator('..')
  const more = section.locator('.moment-more')
  await expect(section.locator('h4')).toHaveText('引用')
  await expect(list.locator('a')).toHaveCount(30)
  await expect.poll(() => references.count()).toBeGreaterThan(3)
  const collapsed = await references.count()
  expect(collapsed).toBeLessThan(30)
  const room = await card.evaluate((element) => {
    const reading = element.querySelector('.moment-reading')!.getBoundingClientRect()
    const context = element.querySelector('.moment-context')!
    const toggle = context.querySelector('.moment-more')!.getBoundingClientRect()
    const row = context.querySelector('.moment-link-list a')!.getBoundingClientRect()
    return {
      spare: reading.bottom - toggle.bottom - parseFloat(getComputedStyle(context).paddingBottom),
      row: row.height,
    }
  })
  expect(room.spare).toBeGreaterThanOrEqual(-1)
  expect(room.spare).toBeLessThan(room.row)
  await expect(list.locator('.lucide-arrow-up-right')).toHaveCount(0)
  await more.click()
  await expect(references).toHaveCount(30)
  await more.click()
  await expect(references).toHaveCount(collapsed)
  await card
    .locator('.moment-reading-actions')
    .getByRole('button', { name: '阅读全文', exact: true })
    .click()
  await expect(references).toHaveCount(30)
  await expect(more).toBeHidden()
  await card
    .locator('.moment-reading-actions')
    .getByRole('button', { name: '收起', exact: true })
    .click()
  await expect(references).toHaveCount(collapsed)
  await page.setViewportSize({ width: 390, height: 900 })
  await expect.poll(() => references.count()).toBeGreaterThan(collapsed)
  expect((await references.count()) % 2).toBe(0)
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)).toBe(false)
  await page.screenshot({ path: testInfo.outputPath('references-narrow.png') })
  await page.setViewportSize({ width: 1440, height: 900 })
  await expect(references).toHaveCount(collapsed)
  await page.screenshot({ path: testInfo.outputPath('references-desktop.png') })
})

test('late report hydration above the reading position preserves the visible anchor', async ({
  page,
}) => {
  const { runs } = await momentsFixture(page, { count: 40, daySize: 20 })
  runs[8].result =
    '# Taller context\n\n' +
    Array.from(
      { length: 12 },
      (_, n) => `## Heading ${n}\n\n[File ${n}](/workspace/report-${n}.md)`,
    ).join('\n\n')
  let release!: () => void
  const gate = new Promise<void>((resolve) => {
    release = resolve
  })
  await page.route(`**/runs/${id(9)}`, async (route) => {
    await gate
    await route.fallback()
  })
  await page.goto(workspacePage('/'))
  const target = page.locator(`[data-moment-id="${id(10)}"]`)
  await target.evaluate((el) => el.scrollIntoView({ block: 'start' }))
  await expect(target.locator('.moment-report')).toBeVisible()
  await page.waitForTimeout(300)
  const top = await target.evaluate((el) => el.getBoundingClientRect().top)
  release()
  await expect(page.locator(`[data-moment-id="${id(9)}"] .moment-context`)).toContainText(
    'Heading 0',
  )
  await expect
    .poll(() => target.evaluate((el) => el.getBoundingClientRect().top))
    .toBeCloseTo(top, 0)
})

test('a DAG output whose shared preview budget ran out loads as pending rather than empty', async ({
  page,
}) => {
  const { runs } = await momentsFixture(page, { count: 1 })
  runs[0].config.dag = { nodes: [] }
  runs[0].result = {
    type: 'dag',
    outputs: [
      { node_id: 'a', name: 'Same name', state: 'succeeded', result: '# A report' },
      { node_id: 'b', name: 'Same name', state: 'succeeded', result: '# B report' },
    ],
  }
  let release!: () => void
  const waiting = new Promise<void>((resolve) => {
    release = resolve
  })
  await page.route(`**/runs/${id(1)}`, async (route) => {
    await waiting
    await route.fallback()
  })
  await page.route('**/moments?**', (route) =>
    route.fulfill({
      json: {
        timezone: 'UTC',
        date: '2026-10-21',
        next_cursor: null,
        items: [
          {
            ...runs[0],
            name: 'DAG budget fixture',
            format: 'dag',
            node_count: 2,
            preview: null,
            outputs: [
              {
                node_id: 'a',
                name: 'Same name',
                state: 'succeeded',
                preview: '# A preview',
                format: 'markdown',
                has_more: true,
              },
              {
                node_id: 'b',
                name: 'Same name',
                state: 'succeeded',
                preview: '',
                format: 'markdown',
                has_more: true,
              },
            ],
            has_more: true,
          },
        ],
      },
    }),
  )
  await page.goto(workspacePage('/'))
  await page.getByRole('button', { name: 'Same name · b', exact: true }).click()
  await expect(page.locator('.moment-report')).toContainText('加载完整结果')
  await expect(page.locator('.moment-report')).not.toContainText('未返回结果')
  await expect(page.locator('.moment-output-id')).toHaveText(['a', 'b'])
  release()
  await expect(page.getByRole('heading', { name: 'B report', exact: true })).toBeVisible()
})
