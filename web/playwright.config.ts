import { defineConfig } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'
const file = process.env.WAKEIT_TEST_STACK ?? path.resolve('../.local/integration-stack.json')
const stack = fs.existsSync(file)
  ? JSON.parse(fs.readFileSync(file, 'utf8'))
  : { base_url: process.env.WAKEIT_TEST_URL ?? 'http://127.0.0.1:8080' }
export default defineConfig({
  testDir: './tests',
  fullyParallel: false,
  workers: 1,
  timeout: 30000,
  use: {
    baseURL: process.env.WAKEIT_TEST_URL ?? stack.base_url,
    locale: 'zh-CN',
    viewport: { width: 1440, height: 900 },
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  reporter: [['list'], ['html', { open: 'never' }]],
})
