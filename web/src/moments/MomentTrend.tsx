import { useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { MomentTrendDay } from '../api/types'
import { TooltipTarget } from '../components/ui'
import { locale } from '../i18n'

export function MomentTrend({ days }: { days: MomentTrendDay[] }) {
  const { t } = useTranslation()
  const root = useRef<SVGSVGElement>(null)
  const [width, setWidth] = useState(300),
    [active, setActive] = useState<number | null>(null)
  useLayoutEffect(() => {
    if (!root.current) return
    const observer = new ResizeObserver(([entry]) => {
      if (entry.contentRect.width) setWidth(entry.contentRect.width)
    })
    observer.observe(root.current)
    return () => observer.disconnect()
  }, [])
  const left = 30,
    right = width - 16,
    top = 18,
    bottom = 122
  const maximum = Math.max(2, ...days.flatMap((day) => [day.succeeded, day.failed]))
  const ceiling = maximum <= 10 ? Math.ceil(maximum / 2) * 2 : Math.ceil(maximum / 10) * 10
  const step = (right - left) / Math.max(1, days.length - 1)
  const x = (i: number) => left + i * step,
    y = (value: number) => bottom - (value / ceiling) * (bottom - top)
  function path(key: 'succeeded' | 'failed') {
    return days
      .map((day, i) => {
        if (!i) return `M ${x(0)} ${y(day[key])}`
        const middle = (x(i - 1) + x(i)) / 2
        return `C ${middle} ${y(days[i - 1][key])}, ${middle} ${y(day[key])}, ${x(i)} ${y(day[key])}`
      })
      .join(' ')
  }
  return (
    <svg
      ref={root}
      className="moment-trend"
      viewBox={`0 0 ${width} 154`}
      role="img"
      aria-label={t('moments.trend')}
    >
      <text x={0} y={9}>
        {t('moments.times')}
      </text>
      {[0, ceiling / 2, ceiling].map((value) => (
        <g key={value}>
          <line className="moment-trend-grid" x1={left} x2={right} y1={y(value)} y2={y(value)} />
          <text x={left - 8} y={y(value) + 4} textAnchor="end">
            {value.toLocaleString(locale())}
          </text>
        </g>
      ))}
      {days.length > 0 && (
        <>
          <path
            className="moment-trend-area"
            d={`${path('succeeded')} L ${x(days.length - 1)} ${bottom} L ${left} ${bottom} Z`}
          />
          <path className="moment-trend-line success" d={path('succeeded')} />
          <path className="moment-trend-line failure" d={path('failed')} />
        </>
      )}
      {days.map((day, i) => {
        const description = t('moments.trendDay', {
          date: day.date,
          succeeded: day.succeeded,
          failed: day.failed,
        })
        return (
          <g key={day.date}>
            <text x={x(i)} y={145} textAnchor="middle">
              {day.date.slice(5)}
            </text>
            <TooltipTarget label={description}>
              <rect
                fill="transparent"
                x={Math.max(left, x(i) - step / 2)}
                y={top}
                width={i === 0 || i === days.length - 1 ? step / 2 : step}
                height={bottom - top + 6}
                tabIndex={0}
                role="img"
                aria-label={description}
                onPointerEnter={() => setActive(i)}
                onPointerLeave={() => setActive(null)}
                onFocus={() => setActive(i)}
                onBlur={() => setActive(null)}
              />
            </TooltipTarget>
            {active === i && (
              <g className="moment-trend-markers">
                <line className="moment-trend-grid" x1={x(i)} x2={x(i)} y1={top} y2={bottom} />
                <circle className="success" cx={x(i)} cy={y(day.succeeded)} r={3} />
                <circle className="failure" cx={x(i)} cy={y(day.failed)} r={3} />
              </g>
            )}
          </g>
        )
      })}
    </svg>
  )
}
