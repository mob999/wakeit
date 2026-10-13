import { pinyin } from 'pinyin-pro'
export function taskIdSuffix(): string {
  return Array.from(crypto.getRandomValues(new Uint8Array(5)), (b) =>
    b.toString(16).padStart(2, '0'),
  ).join('')
}
export function createTaskId(name: string, suffix: string): string {
  const prefix =
    pinyin(name.trim(), { toneType: 'none', type: 'array', nonZh: 'consecutive' })
      .join('-')
      .normalize('NFKD')
      .replace(/[\u0300-\u036f]/g, '')
      .replace(/[^A-Za-z0-9_-]+/g, '-')
      .replace(/-+/g, '-')
      .replace(/^[-_]+|[-_]+$/g, '')
      .slice(0, 69)
      .replace(/[-_]+$/g, '') || 'task'
  return `${prefix}-${suffix}`
}
