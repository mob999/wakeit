import { t } from '../i18n'
function parts(epoch: number, timezone: string) {
  const p = Object.fromEntries(
    new Intl.DateTimeFormat('en-CA', {
      timeZone: timezone,
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hourCycle: 'h23',
    })
      .formatToParts(epoch)
      .map((p) => [p.type, p.value]),
  )
  return [
    Number(p.year),
    Number(p.month),
    Number(p.day),
    Number(p.hour),
    Number(p.minute),
    Number(p.second),
  ]
}
const stamp = (p: number[]) => Date.UTC(p[0], p[1] - 1, p[2], p[3], p[4], p[5])
export function localDateTime(iso: string, timezone: string) {
  const p = parts(Date.parse(iso), timezone),
    pad = (n: number) => String(n).padStart(2, '0')
  return (
    p[0] + '-' + pad(p[1]) + '-' + pad(p[2]) + 'T' + pad(p[3]) + ':' + pad(p[4]) + ':' + pad(p[5])
  )
}
export function utcDateTime(local: string, timezone: string) {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?$/.exec(local)
  if (!match) throw new Error(t('dag.invalidDate'))
  const input = match.slice(1).map((v) => Number(v || 0)),
    naive = stamp(input),
    offsets = new Set<number>()
  try {
    for (let h = -36; h <= 36; h += 6) {
      const sample = naive + h * 3600000
      offsets.add(stamp(parts(sample, timezone)) - sample)
    }
  } catch {
    throw new Error(t('dag.invalidTimezone'))
  }
  const candidates = [...offsets]
    .map((offset) => naive - offset)
    .filter((epoch) => parts(epoch, timezone).every((value, i) => value === input[i]))
  if (!candidates.length) throw new Error(t('dag.invalidDate'))
  return new Date(Math.min(...candidates)).toISOString()
}
export function utcOffset(iso: string, timezone: string) {
  return (
    new Intl.DateTimeFormat('en', { timeZone: timezone, timeZoneName: 'shortOffset' })
      .formatToParts(new Date(iso))
      .find((p) => p.type === 'timeZoneName')?.value ?? ''
  )
}
