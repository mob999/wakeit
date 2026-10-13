import { type Page, type Locator } from '@playwright/test'
const labels: Record<string, string> = {
  cron: 'Cron',
  every: '固定间隔',
  at: '一次性执行',
  manual: '手动执行',
  pi: 'Pi',
  latest: '补最近一次',
  skip: '跳过',
}
export async function selectChoice(page: Page, scope: Locator, label: string, value: string) {
  await scope.getByRole('combobox', { name: label, exact: true }).click()
  const name = labels[value] ?? value
  await page
    .getByRole('option', {
      name: new RegExp('^' + name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') + '(?:\\s|$)'),
    })
    .click()
}
