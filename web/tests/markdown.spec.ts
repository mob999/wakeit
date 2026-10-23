import { workspacePage, apiPath, mockWorkspace } from './workspace'
import { test, expect, type Page, type Locator } from '@playwright/test'

const id = '22222222-3333-4444-8555-666666666666'
const report = [
  '# 渲染能力',
  '<span style="color:#ef4444">红色文字</span> <span style="background-color:#22c55e;font-weight:bold">绿色背景</span>',
  '内联公式：$E=mc^2$。',
  '$$\n\\int_0^1 x^2\\,dx=\\frac{1}{3}\n$$',
  '```rust\nfn main() { println!("中文"); }\n```',
  '```diff\n- 删除旧内容\n+ 添加新内容\n  保留内容\n```',
  '```html\n<span style="color:red">代码示例保持原样</span>\n```',
  '```wakeit-unknown-language\nunknown syntax: 保持原文\n```',
  '```mermaid\nflowchart LR\nA[任务] --> B[执行] --> C[结果]\n```',
  '<details><summary>更多信息</summary>折叠内容</details>',
  '<script>window.markdownExecuted=true</script>',
  '<span style="position:fixed;background-image:url(https://invalid.example/style);color:var(--secret);font-size:999px" onclick="window.markdownExecuted=true">清理样式</span>',
  '<iframe srcdoc="<script>parent.markdownExecuted=true</script>"></iframe>',
  '[危险链接](javascript:alert(1))',
].join('\n\n')
async function fixture(page: Page, text = report) {
  const run = {
    id,
    task_id: 'markdown-task',
    state: 'succeeded',
    source: 'manual',
    created_at: '2026-10-15T04:00:00Z',
    started_at: '2026-10-15T04:00:00Z',
    finished_at: '2026-10-15T04:01:00Z',
    result: text,
    cancel_requested: false,
    config: {
      version: 1,
      id: 'markdown-task',
      name: 'Markdown 报告',
      runner: 'dev',
      agent: 'pi',
      cwd: '/project',
      prompt: 'report',
      parameters: {},
      enabled: false,
      schedule: null,
      execution: { timeout: '1h', start_grace: '1h', overlap: 'skip', misfire: 'latest' },
    },
    execution_snapshot: {
      cwd: '/project',
      model: 'Agent 默认',
      parameters: {},
      trace_supported: true,
    },
  }
  await page.route('**/api/v1/**', async (route) => {
    if (await mockWorkspace(route, 'admin')) return
    const url = new URL(route.request().url())
    const path = apiPath(url.pathname)
    if (path === '/session')
      return route.fulfill({ json: { id: 'tester', username: 'tester', role: 'admin' } })
    if (path === '/meta') return route.fulfill({ json: { version: '0.1.0' } })
    if (path === '/settings')
      return route.fulfill({
        json: { timezone: 'Asia/Shanghai', log_retention_days: 7, run_retention_days: 90 },
      })
    if (path === '/agents' || path === '/runners') return route.fulfill({ json: [] })
    if (path === '/runs')
      return route.fulfill({ json: { items: [run], total: 1, page: 1, page_size: 20 } })
    if (path === `/runs/${id}`) return route.fulfill({ json: run })
    if (path.endsWith('/stream'))
      return route.fulfill({ contentType: 'text/event-stream', body: ':ready\n\n' })
    if (path.endsWith('/events'))
      return route.fulfill({
        json: [
          {
            id: 1,
            seq: 1,
            kind: 'trace',
            created_at: run.started_at,
            data: {
              id: 'message',
              kind: 'message',
              title: '报告消息',
              status: 'succeeded',
              output: text,
              occurred_at: run.started_at,
            },
          },
        ],
      })
    if (path === '/moments/days')
      return route.fulfill({
        json: {
          timezone: 'Asia/Shanghai',
          today: '2026-10-15',
          total_runs: 1,
          days: [{ date: '2026-10-15', count: 1 }],
          next_before: null,
        },
      })
    if (path === '/moments')
      return route.fulfill({
        json: {
          timezone: 'Asia/Shanghai',
          date: '2026-10-15',
          next_cursor: null,
          items: [
            {
              id,
              task_id: run.task_id,
              name: run.config.name,
              state: run.state,
              finished_at: run.finished_at,
              runner: 'dev',
              agent: 'Pi',
              preview: text,
              format: 'markdown',
              has_more: false,
              truncated: false,
            },
          ],
        },
      })
    return route.fulfill({ json: [] })
  })
  return run
}
async function assertReport(region: Locator) {
  await expect(region.getByText('红色文字', { exact: true })).toHaveCSS('color', 'rgb(239, 68, 68)')
  await expect(region.getByText('绿色背景', { exact: true })).toHaveCSS(
    'background-color',
    'rgb(34, 197, 94)',
  )
  await expect(region.locator('.katex')).toHaveCount(2)
  await expect(region.locator('.katex-display')).toHaveCount(1)
  await expect(region.locator('code.language-rust .hljs-keyword')).toContainText('fn')
  await expect(region.locator('code.language-diff .hljs-addition')).toContainText('添加新内容')
  await expect(region.locator('code.language-diff .hljs-deletion')).toContainText('删除旧内容')
  const addition = await region.locator('.hljs-addition').evaluate((e) => getComputedStyle(e).color)
  const deletion = await region.locator('.hljs-deletion').evaluate((e) => getComputedStyle(e).color)
  expect(addition).not.toBe(deletion)
  await expect(region.locator('code.language-html')).toContainText('<span style="color:red">')
  await expect(region.locator('code.language-html span[style]')).toHaveCount(0)
  await expect(region.locator('code.language-wakeit-unknown-language')).toContainText('保持原文')
}

test('Run results, trace messages and Moments share syntax, math, HTML and Mermaid rendering', async ({
  page,
}) => {
  await fixture(page)
  await page.goto(workspacePage(`/runs?run=${id}`))
  const result = page.locator('.run-result-panel .markdown-result')
  await assertReport(result)
  const diagram = result.locator('iframe[title="Mermaid 图表"]')
  await expect(diagram).toBeVisible({ timeout: 30000 })
  await expect(diagram).toHaveAttribute('sandbox', '')
  await expect(diagram.contentFrame().locator('svg')).toBeVisible()
  await expect(diagram.contentFrame().locator('body')).toContainText('任务')
  await expect(diagram.contentFrame().locator('body')).toContainText('结果')
  await page.getByRole('tab', { name: '轨迹', exact: true }).click()
  await page.locator('.trace-row[data-trace-id="message"] > summary').click()
  await assertReport(page.locator('.trace-message'))
  await page.goto(workspacePage('/'))
  await page.locator('.moment-card').getByRole('button', { name: '阅读全文', exact: true }).click()
  await assertReport(page.locator('.report-content'))
  await expect(page.locator('.report-content iframe')).toBeVisible()
})

test('raw HTML and diagram input cannot inject scripts, links or layout CSS; raw view stays literal', async ({
  page,
}) => {
  await fixture(page)
  const probes: string[] = []
  page.on('request', (request) => {
    if (request.url().includes('invalid.example')) probes.push(request.url())
  })
  await page.goto(workspacePage(`/runs?run=${id}`))
  const result = page.locator('.run-result-panel .markdown-result')
  await expect(
    result.locator('script, style, iframe:not([sandbox]), [onclick], a[href^="javascript:"]'),
  ).toHaveCount(0)
  await expect(result.getByText('清理样式', { exact: true })).not.toHaveAttribute('style')
  expect(await page.evaluate(() => (window as any).markdownExecuted)).toBeUndefined()
  expect(probes).toEqual([])
  await page.getByRole('button', { name: '原文', exact: true }).click()
  await expect(page.locator('.result-raw')).toHaveText(report)
  await expect(page.locator('.result-raw script, .result-raw iframe')).toHaveCount(0)
  expect(await page.evaluate(() => (window as any).markdownExecuted)).toBeUndefined()
})

test('malformed diagrams and formulas retain readable source without breaking the rest of the report', async ({
  page,
}) => {
  await fixture(
    page,
    '# 仍可阅读\n\n```mermaid\nflowchart LR\nA[未闭合\n```\n\n$$\n\\notarealcommand{a}\n$$\n\n结尾正文',
  )
  await page.goto(workspacePage(`/runs?run=${id}`))
  const result = page.locator('.run-result-panel .markdown-result')
  await expect(result.getByRole('heading', { name: '仍可阅读' })).toBeVisible()
  await expect(page.getByRole('alert')).toContainText('图表无法渲染', { timeout: 30000 })
  await expect(result.locator('.mermaid-diagram pre')).toContainText('A[未闭合')
  await expect(result.locator('.katex')).toBeVisible()
  await expect(result.locator('.katex')).toContainText('notarealcommand')
  await expect(result).toContainText('结尾正文')
  await expect(page.locator('body > div[id^="dwakeit-diagram"]')).toHaveCount(0)
})

test('Markdown extensions follow light and dark themes without overflowing 390px, desktop or wide layouts', async ({
  page,
}) => {
  await fixture(page)
  await page.goto(workspacePage(`/runs?run=${id}`))
  for (const [width, theme] of [
    [1440, 'light'],
    [1440, 'dark'],
    [390, 'dark'],
    [2200, 'light'],
  ] as const) {
    await page.setViewportSize({ width, height: 1000 })
    await page.evaluate((value) => {
      document.documentElement.dataset.theme = value
    }, theme)
    const result = page.locator('.run-result-panel .markdown-result')
    await expect(result.locator('iframe')).toBeVisible({ timeout: 30000 })
    await expect(result.locator('.mermaid-diagram')).toHaveAttribute('aria-busy', 'false')
    expect(
      await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
    ).toBeTruthy()
    const frame = result.locator('iframe')
    const bounds = (await frame.boundingBox())!
    expect(bounds.x + bounds.width).toBeLessThanOrEqual(width)
    await expect(frame.contentFrame().locator('body')).toHaveCSS(
      'background-color',
      theme === 'dark' ? 'rgb(10, 10, 10)' : 'rgb(255, 255, 255)',
    )
    await page.screenshot({ path: `test-results/markdown-${width}-${theme}.png`, fullPage: true })
  }
})

test('diagram dependencies load only when needed and download failures preserve source and report content', async ({
  page,
}) => {
  await fixture(page, '# 普通报告\n\n```rust\nfn main() {}\n```')
  const loads: string[] = []
  page.on('request', (request) => {
    if (/\/mermaid\.js(?:\?|$)|\/assets\/mermaid(?:\.core)?-[^/]+\.js/.test(request.url()))
      loads.push(request.url())
  })
  await page.goto(workspacePage(`/runs?run=${id}`))
  await expect(page.locator('.run-result-panel .markdown-result h1')).toHaveText('普通报告')
  expect(loads).toEqual([])
  await page.unrouteAll({ behavior: 'wait' })
  await fixture(page, '# 图表报告\n\n```mermaid\nflowchart LR\nA --> B\n```\n\n其余正文')
  await page.route(/\/mermaid\.js(?:\?|$)|\/assets\/mermaid(?:\.core)?-[^/]+\.js/, (route) =>
    route.abort(),
  )
  await page.reload()
  await expect(page.getByRole('alert')).toContainText('图表无法渲染', { timeout: 30000 })
  await expect(page.locator('.run-result-panel .mermaid-diagram pre')).toContainText('A --> B')
  await expect(page.locator('.run-result-panel .markdown-result')).toContainText('其余正文')
})
