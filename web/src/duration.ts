export function validDuration(value: string) {
  const units: Record<string, number> = {
    ns: 1e-9,
    nsec: 1e-9,
    nanos: 1e-9,
    us: 1e-6,
    usec: 1e-6,
    µs: 1e-6,
    ms: 0.001,
    msec: 0.001,
    millis: 0.001,
    s: 1,
    sec: 1,
    secs: 1,
    second: 1,
    seconds: 1,
    m: 60,
    min: 60,
    mins: 60,
    minute: 60,
    minutes: 60,
    h: 3600,
    hr: 3600,
    hrs: 3600,
    hour: 3600,
    hours: 3600,
    d: 86400,
    day: 86400,
    days: 86400,
    w: 604800,
    wk: 604800,
    wks: 604800,
    week: 604800,
    weeks: 604800,
    M: 2630016,
    month: 2630016,
    months: 2630016,
    y: 31557600,
    yr: 31557600,
    yrs: 31557600,
    year: 31557600,
    years: 31557600,
  }
  let seconds = 0
  const rest = value.trim().replace(/(\d+(?:\.\d+)?)\s*([a-zMµ]+)/g, (_part, number, unit) => {
    seconds += Number(number) * (units[unit] ?? NaN)
    return ''
  })
  return !rest.trim() && Number.isFinite(seconds) && seconds >= 1 && seconds <= 366 * 86400
}
