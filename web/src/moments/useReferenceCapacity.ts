import { useLayoutEffect, useState, type RefObject } from 'react'

// Measure the reading column, not the grid's stretched height: letting the
// reference list set its own budget would grow the card on every measurement.
export function useReferenceCapacity({
  reading,
  context,
  list,
  toggle,
  count,
}: {
  reading: RefObject<HTMLDivElement | null>
  context: RefObject<HTMLElement | null>
  list: RefObject<HTMLElement | null>
  toggle: RefObject<HTMLButtonElement | null>
  count: number
}) {
  const [capacity, setCapacity] = useState(1)
  useLayoutEffect(() => {
    const body = reading.current,
      aside = context.current,
      rows = list.current
    if (!body || !aside || !rows || !count) return
    const measure = () => {
      const row = rows.querySelector<HTMLElement>('a,button')
      if (!row) return
      const bodyRect = body.getBoundingClientRect(),
        asideRect = aside.getBoundingClientRect()
      const styles = getComputedStyle(rows),
        asideStyles = getComputedStyle(aside)
      const columns = styles.display === 'grid' ? styles.gridTemplateColumns.split(' ').length : 1
      const rowHeight = row.getBoundingClientRect().height
      if (!rowHeight) return
      const gap = parseFloat(styles.rowGap) || 0
      const heading = rows.previousElementSibling as HTMLElement | null
      const headingSpace = heading
        ? heading.getBoundingClientRect().height +
          (parseFloat(getComputedStyle(heading).marginBottom) || 0)
        : 0
      const beside = asideRect.left >= bodyRect.right - 1 || asideRect.right <= bodyRect.left + 1
      const available = Math.max(
        0,
        beside
          ? bodyRect.bottom -
              rows.getBoundingClientRect().top -
              (parseFloat(asideStyles.paddingBottom) || 0)
          : bodyRect.height - headingSpace,
      )
      const allRows = Math.ceil(count / columns)
      if (allRows * rowHeight + (allRows - 1) * gap <= available + 0.5) {
        setCapacity(count)
        return
      }
      const control = toggle.current
      const controlStyles = control && getComputedStyle(control)
      const controlHeight = controlStyles
        ? parseFloat(controlStyles.lineHeight) +
          parseFloat(controlStyles.paddingTop) +
          parseFloat(controlStyles.paddingBottom) +
          (parseFloat(controlStyles.marginTop) || 0)
        : rowHeight
      const fittingRows = Math.max(
        1,
        Math.floor((available - controlHeight + gap) / (rowHeight + gap)),
      )
      setCapacity(Math.min(count, fittingRows * columns))
    }
    measure()
    const observer = new ResizeObserver(measure)
    observer.observe(body)
    observer.observe(aside)
    for (const section of aside.children) observer.observe(section)
    return () => observer.disconnect()
  }, [count, reading, context, list, toggle])
  return capacity
}
