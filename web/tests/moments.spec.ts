import { test, expect, type Page } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'
import { loginSession } from './session'
const stack = JSON.parse(
  fs.readFileSync(
    process.env.WAKEIT_TEST_STACK ?? path.resolve('../.local/integration-stack.json'),
    'utf8',
  ),
)
const id = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, '0')}`
function entry(n: number, extra = {}) {
  return {
    id: id(n),
    task_id: 'moment-task',
    name: `报告 ${n}`,
    state: 'succeeded',
    reason: null,
    finished_at: '2026-10-15T06:32:00Z',
    runner: '开发设备',
    agent: 'Pi',
    logo: '/agent-logos/pi.svg',
    preview: '# 今日报告\n\n检查完成。\n\n| 项目 | 状态 |\n|---|---|\n| 测试 | 通过 |',
    format: 'markdown',
    has_more: false,
    truncated: false,
    ...extra,
  }
}
async function fixture(
  page: Page,
  options: { empty?: boolean; single?: boolean; error?: boolean } = {},
) {
  const requests: string[] = []
  let directoryFailed = options.error ?? false
  await page.route('**/api/v1/moments**', async (route) => {
    const url = new URL(route.request().url())
    requests.push(url.pathname + url.search)
    if (url.pathname.endsWith('/days')) {
      if (directoryFailed) {
        directoryFailed = false
        return route.fulfill({ status: 503, json: { error: '暂时无法加载动态' } })
      }
      const before = url.searchParams.get('before')
      return route.fulfill({
        json: {
          timezone: 'Asia/Shanghai',
          today: '2026-10-15',
          total_runs: options.empty ? 0 : options.single ? 1 : 25,
          days: options.empty
            ? []
            : options.single
              ? [{ date: '2026-10-13', count: 1 }]
              : before
                ? [{ date: '2026-10-12', count: 1 }]
                : [
                    { date: '2026-10-15', count: 22 },
                    { date: '2026-10-14', count: 1 },
                    { date: '2026-10-13', count: 1 },
                  ],
          next_before: options.empty || options.single || before ? null : '2026-10-13',
        },
      })
    }
    const date = url.searchParams.get('date')
    const cursor = url.searchParams.get('cursor')
    const items = options.single
      ? [entry(13)]
      : date === '2026-10-15'
        ? cursor
          ? [entry(21), entry(22)]
          : [
              entry(1, { has_more: true, preview: '# 长报告\n\n' + '预览正文。'.repeat(420) }),
              entry(2, { state: 'failed', reason: '命令退出码 1', preview: '部分分析结果' }),
              entry(3, { format: 'json', preview: '{\n  "ok": true\n}' }),
              entry(4, { truncated: true, preview: '保留的报告' }),
              entry(5, { preview: null }),
              ...Array.from({ length: 15 }, (_, i) => entry(i + 6)),
            ]
        : [entry(date === '2026-10-14' ? 23 : date === '2026-10-13' ? 24 : 25)]
    return route.fulfill({
      json: {
        timezone: 'Asia/Shanghai',
        date,
        items,
        next_cursor: date === '2026-10-15' && !cursor ? 'next' : null,
      },
    })
  })
  await page.route(`**/api/v1/runs/${id(1)}`, (route) =>
    route.fulfill({
      json: {
        id: id(1),
        task_id: 'moment-task',
        state: 'succeeded',
        reason: null,
        source: 'manual',
        created_at: '2026-10-15T05:00:00Z',
        scheduled_at: '2026-10-15T05:00:00Z',
        started_at: '2026-10-15T05:00:00Z',
        finished_at: '2026-10-15T06:32:00Z',
        result: '# 完整报告\n\n' + '完整正文。'.repeat(800) + '\n\n全文结束',
        config: {
          version: 1,
          id: 'moment-task',
          name: '报告 1',
          runner: '开发设备',
          agent: 'Pi',
          cwd: '/project',
          prompt: 'check',
          parameters: {},
          enabled: true,
          schedule: null,
          execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
        },
        execution_snapshot: null,
        task_archived: false,
        cancel_requested: false,
        job_id: null,
      },
    }),
  )
  return requests
}
test('Moments is the default home for all roles; Tasks keeps its own route', async ({ page }) => {
  for (const user of ['admin', 'operator', 'viewer']) {
    await page.context().clearCookies()
    await loginSession(page, user, user === 'admin' ? stack.password : 'wakeit-role-test-password')
    await page.getByRole('link', { name: '动态', exact: true }).click()
    await expect(page).toHaveURL(/\/$/)
    await expect(page.getByRole('heading', { name: '动态', exact: true })).toBeVisible()
    await expect(page.locator('.list-pane')).toHaveCount(0)
    await page.getByRole('link', { name: '任务', exact: true }).click()
    await expect(page).toHaveURL(/\/tasks$/)
    await expect(page.locator('.list-pane h1')).toHaveText('任务')
  }
})
test('all days show previews without collapse defaults or a singleton exception', async ({
  page,
}) => {
  await loginSession(page, 'admin', stack.password)
  await fixture(page)
  await page.getByRole('link', { name: '动态', exact: true }).click()
  for (const date of ['2026-10-15', '2026-10-14', '2026-10-13'])
    await expect(page.getByRole('region', { name: date })).toBeVisible()
  await expect(page.locator('.moment-day')).toHaveCount(3)
  await expect(page.locator('.moment-entry')).toHaveCount(22)
  await expect(page.locator('.moment-day').nth(1).locator('.moment-report')).toBeVisible()
  await expect(page.locator('.moment-day').nth(2).locator('.moment-report')).toBeVisible()
  await expect(
    page.locator('.moment-card').first().getByRole('button', { name: '阅读全文' }),
  ).toBeVisible()
  await expect(page.locator('.moment-card').nth(1)).toContainText('命令退出码 1')
  await expect(page.locator('.moment-card').nth(2).locator('pre')).toContainText('"ok": true')
  await expect(page.locator('.moment-card').nth(3)).toContainText('结果已截断')
  await expect(page.locator('.moment-card').nth(4)).toContainText('未返回结果')
  await page.getByRole('button', { name: '加载更多', exact: true }).click()
  await expect(page.locator('.moment-entry')).toHaveCount(24)
  await page.getByRole('button', { name: '加载更早', exact: true }).click()
  await expect(page.locator('.moment-day')).toHaveCount(4)
  await expect(page.locator('.moment-entry')).toHaveCount(25)
  await expect(page.getByRole('button', { name: '加载更多', exact: true })).toHaveCount(0)
})
test('manual refresh only; full report and scroll survive Run navigation and refresh', async ({
  page,
}) => {
  await loginSession(page, 'admin', stack.password)
  const requests = await fixture(page)
  await page.getByRole('link', { name: '动态', exact: true }).click()
  const card = page.locator('.moment-card').first()
  await expect(card.getByRole('button', { name: '阅读全文' })).toBeVisible()
  const count = requests.length
  await page.evaluate(() => {
    window.dispatchEvent(new Event('focus'))
    window.dispatchEvent(new Event('online'))
  })
  await page.waitForTimeout(3300)
  expect(requests.length).toBe(count)
  await card.getByRole('button', { name: '阅读全文' }).click()
  await expect(card.locator('.moment-report')).toContainText('全文结束')
  await expect(card.getByRole('button', { name: '收起', exact: true })).toBeVisible()
  await page.locator('.moments-page').evaluate((e) => (e.scrollTop = 400))
  const scroll = await page.locator('.moments-page').evaluate((e) => e.scrollTop)
  await page.getByRole('button', { name: '刷新动态' }).click()
  await expect.poll(() => requests.length).toBeGreaterThan(count)
  await expect(card.getByRole('button', { name: '收起', exact: true })).toBeVisible()
  await expect.poll(() => page.locator('.moments-page').evaluate((e) => e.scrollTop)).toBe(scroll)
  await page.locator('.moments-page').evaluate((e) => (e.scrollTop = 0))
  await card.getByRole('link', { name: '查看运行' }).click()
  await expect(page).toHaveURL(
    (url) => url.pathname === '/runs' && url.searchParams.get('run') === id(1),
  )
  await page.goBack()
  await expect(card.getByRole('button', { name: '收起', exact: true })).toBeVisible()
  await page.locator('.moments-page').evaluate((e) => (e.scrollTop = 400))
  await page.getByRole('link', { name: '运行', exact: true }).click()
  await page.getByRole('link', { name: '动态', exact: true }).click()
  await expect.poll(() => page.locator('.moments-page').evaluate((e) => e.scrollTop)).toBe(400)
  await page.reload()
  await expect(
    page.locator('.moment-card').first().getByRole('button', { name: '阅读全文' }),
  ).toBeVisible()
  await expect(page.locator('.moments-page')).not.toContainText('全文结束')
})
test('empty state, load retry and historical single result use the same preview presentation', async ({
  page,
}) => {
  await loginSession(page, 'admin', stack.password)
  await fixture(page, { empty: true, error: true })
  await page.getByRole('link', { name: '动态', exact: true }).click()
  await expect(page.getByRole('alert')).toContainText('暂时无法加载动态')
  await page.getByRole('button', { name: '重试', exact: true }).click()
  await expect(page.locator('.moments-page')).toContainText('暂无执行结果')
  await fixture(page, { single: true })
  await page.reload()
  await expect(page.locator('.moment-day')).toHaveCount(1)
  await expect(page.locator('.moment-card .markdown-result table')).toBeVisible()
  await expect(page.getByRole('button', { name: '阅读全文' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: '收起', exact: true })).toHaveCount(0)
})
test('light, dark and narrow timelines keep readable cards, tiny logos and no overflow', async ({
  page,
}) => {
  await loginSession(page, 'admin', stack.password)
  await fixture(page)
  await page.getByRole('link', { name: '动态', exact: true }).click()
  await expect(page.locator('.moment-entry')).toHaveCount(22)
  for (const width of [1440, 2200, 390]) {
    await page.setViewportSize({ width, height: 900 })
    for (const theme of ['light', 'dark']) {
      await page.evaluate((theme) => (document.documentElement.dataset.theme = theme), theme)
      expect(
        await page.locator('.moments-page').evaluate((e) => e.scrollWidth <= e.clientWidth),
      ).toBeTruthy()
      expect(
        await page
          .locator('.moment-report')
          .first()
          .evaluate((e) => e.clientHeight),
      ).toBeLessThanOrEqual(240)
      await expect(page.locator('.moment-card .card-body .moment-identity')).toHaveCount(0)
      await expect(page.locator('.moment-card .card-header .moment-identity').first()).toBeVisible()
      const axis = await page.locator('.moments-timeline').evaluate((e) => {
        const line = getComputedStyle(e, '::before')
        const days = e.querySelectorAll('.moment-date')
        return {
          height: parseFloat(line.height),
          span:
            days[days.length - 1].getBoundingClientRect().top - days[0].getBoundingClientRect().top,
        }
      })
      expect(axis.height).toBeGreaterThan(axis.span)
      const dates = await page.locator('.moment-date').evaluateAll((elements) =>
        elements.map((element) => {
          const title = element.querySelector('h2')!.getBoundingClientRect()
          const count = element.querySelector('.moment-date-label > span')!.getBoundingClientRect()
          const timeline = element.closest('.moments-timeline')!
          const axis =
            timeline.getBoundingClientRect().left +
            parseFloat(getComputedStyle(timeline, '::before').left) +
            0.5
          return {
            center: title.left + title.width / 2,
            axis,
            gap: count.left - title.right,
            dot: getComputedStyle(element, '::before').content,
          }
        }),
      )
      for (const date of dates) {
        expect(Math.abs(date.center - date.axis)).toBeLessThanOrEqual(0.5)
        expect(date.gap).toBeCloseTo(2, 1)
        expect(date.dot).toBe('none')
      }
      const logo = page.locator('.moment-identity .agent-logo').first()
      expect(await logo.evaluate((e) => e.getBoundingClientRect().height)).toBeLessThanOrEqual(14)
      await page.screenshot({ path: `test-results/moments-${width}-${theme}.png` })
    }
  }
})

test('copy reads full result only on demand and reports clipboard failures without losing content', async ({
  page,
  context,
}) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  await loginSession(page, 'admin', stack.password)
  await fixture(page)
  let fullReads = 0
  page.on('request', (r) => {
    if (new URL(r.url()).pathname === `/api/v1/runs/${id(1)}`) fullReads++
  })
  await page.getByRole('link', { name: '动态', exact: true }).click()
  const card = page.locator('.moment-card').first()
  await expect(card.getByRole('button', { name: '阅读全文' })).toBeVisible()
  expect(fullReads).toBe(0)
  await card.getByRole('button', { name: '复制结果' }).click()
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toContain('全文结束')
  expect(fullReads).toBe(1)
  await expect(card.getByRole('button', { name: '阅读全文' })).toBeVisible()
  await page.evaluate(() =>
    Object.defineProperty(navigator.clipboard, 'writeText', {
      configurable: true,
      value: () => Promise.reject(new Error('复制失败')),
    }),
  )
  await card.getByRole('button', { name: '复制结果' }).click()
  await expect(page.getByRole('status')).toContainText('复制失败')
  await expect(card.locator('.moment-report')).toContainText('预览正文')
})

test('full report errors retry in place and workspace timezone changes regroup on refresh', async ({
  page,
}) => {
  await loginSession(page, 'admin', stack.password)
  await fixture(page)
  let fail = true
  await page.route(`**/api/v1/runs/${id(1)}`, async (route) => {
    if (fail) {
      fail = false
      return route.fulfill({ status: 503, json: { error: '全文暂不可用' } })
    }
    return route.fallback()
  })
  await page.getByRole('link', { name: '动态', exact: true }).click()
  const card = page.locator('.moment-card').first()
  await card.getByRole('button', { name: '阅读全文' }).click()
  await expect(page.getByRole('alert')).toContainText('全文暂不可用')
  await card.getByRole('button', { name: '重试', exact: true }).click()
  await expect(card.locator('.moment-report')).toContainText('全文结束')
  await page.route('**/api/v1/moments/days**', (route) =>
    route.fulfill({
      json: {
        timezone: 'UTC',
        today: '2026-10-15',
        total_runs: 1,
        days: [{ date: '2026-10-14', count: 1 }],
        next_before: null,
      },
    }),
  )
  await page.getByRole('button', { name: '刷新动态' }).click()
  await expect(page.locator('.moment-day')).toHaveCount(1)
  await expect(page.locator('.moment-date')).toContainText('昨天')
  await expect(page.locator('.moment-time')).toHaveText('06:32')
})

test('refresh keeps the visible report anchored when a new result is inserted above it', async ({
  page,
}) => {
  await loginSession(page, 'admin', stack.password)
  await fixture(page)
  let added = false
  const rows = Array.from({ length: 20 }, (_, i) =>
    entry(i + 1, { has_more: false, preview: '简短报告' }),
  )
  await page.route('**/api/v1/moments?**', (route) => {
    const url = new URL(route.request().url())
    if (url.searchParams.get('date') !== '2026-10-15') return route.fallback()
    return route.fulfill({
      json: {
        timezone: 'Asia/Shanghai',
        date: '2026-10-15',
        items: added ? [entry(99), ...rows] : rows,
        next_cursor: null,
      },
    })
  })
  await page.getByRole('link', { name: '动态', exact: true }).click()
  const target = page.locator(`[data-moment-id="${id(10)}"]`)
  await target.scrollIntoViewIfNeeded()
  const top = await target.evaluate((e) => e.getBoundingClientRect().top)
  added = true
  await page.getByRole('button', { name: '刷新动态' }).click()
  await expect(page.locator('.moment-entry').first()).toContainText('报告 99')
  await expect
    .poll(async () => Math.abs((await target.evaluate((e) => e.getBoundingClientRect().top)) - top))
    .toBeLessThan(0.5)
})
