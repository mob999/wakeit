import { type Page } from '@playwright/test'
export async function languageAction(page: Page, label: string) {
  const direct = page.getByRole('button', { name: label, exact: true })
  if (await direct.isVisible()) return direct
  const display = page.getByRole('button', { name: /^(显示偏好|Display preferences)$/ })
  if ((await display.getAttribute('aria-expanded')) !== 'true') await display.click()
  return page.getByRole('menuitem', { name: label, exact: true })
}
