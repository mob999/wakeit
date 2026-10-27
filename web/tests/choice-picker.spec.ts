import { test, expect } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'
import { loginSession } from './session'
const stack = JSON.parse(
  fs.readFileSync(
    process.env.WAKEIT_TEST_STACK ?? path.resolve('../.local/integration-stack.json'),
    'utf8',
  ),
)
test('shared choices search, support keyboard navigation and preserve offline selection', async ({
  page,
}) => {
  await loginSession(page, 'admin', stack.password)
  const runners = await (await page.request.get('/api/v1/runners')).json()
  const source = runners.find((r: any) => r.name === 'test-device')
  const agents = [
    {
      ...source.agents[0],
      id: 'pi',
      name: 'Pi',
      logo: '/agent-logos/pi.svg',
      available: true,
      models_supported: false,
      parameters: {
        model: { type: 'string', enum: [] },
        reasoning: { type: 'string', enum: ['low', 'high'] },
      },
    },
    {
      ...source.agents[0],
      id: 'codex',
      name: 'Codex',
      available: true,
      logo: '/agent-logos/codex.svg',
    },
    { ...source.agents[0], id: 'claude-code', name: 'Claude Code', available: false },
  ]
  await page.route('**/api/v1/runners', (route) =>
    route.fulfill({
      json: [
        { ...source, name: 'Desktop', online: true, agents },
        { ...source, id: 'offline', name: 'Remote', online: false, agents },
        { ...source, id: 'revoked', name: 'Revoked', revoked: true },
      ],
    }),
  )
  await page.locator('.pane-heading').getByRole('button', { name: '新建任务', exact: true }).click()
  await page.getByRole('menuitem', { name: '任务', exact: true }).click()
  const dialog = page.getByRole('dialog')
  const runner = dialog.getByRole('combobox', { name: 'Runner', exact: true })
  await expect(dialog.getByRole('combobox', { name: 'Agent', exact: true })).toBeDisabled()
  await runner.focus()
  await page.keyboard.press('ArrowDown')
  const search = page.getByRole('combobox', { name: '搜索Runner', exact: true })
  await expect(search).toBeFocused()
  await search.fill('not-found')
  await expect(page.getByText('无匹配选项', { exact: true })).toBeVisible()
  await search.fill('Remote')
  await page.keyboard.press('Enter')
  await expect(runner).toContainText('Remote')
  await expect(runner).toBeFocused()
  await dialog.getByRole('combobox', { name: 'Agent', exact: true }).click()
  await expect(page.getByRole('option', { name: /Claude Code/ })).toHaveCount(0)
  await page.getByLabel('搜索Agent', { exact: true }).fill('claude-code')
  await expect(page.getByText('无匹配选项', { exact: true })).toBeVisible()
  await page.getByLabel('搜索Agent', { exact: true }).fill('pi')
  await expect(page.getByRole('option', { name: /Pi/ })).not.toContainText('未检测到')
  await expect(page.getByRole('option', { name: /Pi/ }).locator('.agent-logo img')).toHaveAttribute(
    'src',
    '/agent-logos/pi.svg',
  )
  await page.keyboard.press('Enter')
  await expect(dialog.getByRole('combobox', { name: 'Agent', exact: true })).toContainText('Pi')
  await expect(dialog.getByRole('textbox', { name: '模型', exact: true })).toBeVisible()
  await dialog.locator('.advanced-settings:visible > summary').click()
  await dialog.getByRole('combobox', { name: 'reasoning', exact: true }).click()
  await expect(page.getByRole('listbox', { name: '可选reasoning' })).toBeFocused()
  await page.keyboard.press('End')
  await page.keyboard.press('Enter')
  await expect(dialog.getByRole('combobox', { name: 'reasoning', exact: true })).toContainText(
    'high',
  )
  await runner.click()
  await expect(page.getByRole('option', { name: 'Revoked', exact: true })).toHaveCount(0)
  await page.getByLabel('搜索Runner').fill('Desktop')
  await page.keyboard.press('Escape')
  await expect(runner).toBeFocused()
  await expect(runner).toContainText('Remote')
  await runner.click()
  await page.getByRole('option', { name: 'Desktop', exact: true }).click()
  await expect(dialog.getByRole('combobox', { name: 'Agent', exact: true })).toContainText(
    '选择 Agent',
  )
})
test('choice popovers and focus borders fit dark and narrow task editors', async ({ page }) => {
  await loginSession(page, 'admin', stack.password)
  await page.locator('.pane-heading').getByRole('button', { name: '新建任务', exact: true }).click()
  await page.getByRole('menuitem', { name: '任务', exact: true }).click()
  const dialog = page.getByRole('dialog')
  for (const [width, dark] of [
    [1440, false],
    [1440, true],
    [390, true],
  ] as const) {
    await page.setViewportSize({ width, height: 900 })
    await page.evaluate(
      (dark) => (document.documentElement.dataset.theme = dark ? 'dark' : 'light'),
      dark,
    )
    const control = dialog.getByRole('combobox', { name: 'Runner', exact: true })
    await control.click()
    const menu = page.locator('.choice-popover')
    await expect(menu).toBeVisible()
    const box = (await menu.boundingBox())!
    const trigger = (await control.boundingBox())!
    expect(Math.abs(box.width - trigger.width)).toBeLessThan(2)
    expect(box.x).toBeGreaterThanOrEqual(0)
    expect(box.x + box.width).toBeLessThanOrEqual(width)
    await page.screenshot({ path: `test-results/choice-${width}-${dark ? 'dark' : 'light'}.png` })
    await page.keyboard.press('Escape')
    expect(await control.evaluate((e) => getComputedStyle(e).outlineStyle)).toBe('none')
  }
})
