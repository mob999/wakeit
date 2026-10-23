type MomentPosition = { id: string; top: number; bottom: number }
// The leading reading edge determines which card the reader has reached.
// Merely seeing the next card at the bottom does not leave the current report.
export function momentCollapseAnchor(
  current: string,
  positions: MomentPosition[],
  viewportTop: number,
  originalOffset: number,
  headerHeight: number,
) {
  const visible = positions.find((position) => position.bottom > viewportTop + headerHeight)
  return visible && visible.id !== current
    ? { id: visible.id, top: visible.top }
    : { id: current, top: viewportTop + Math.max(0, originalOffset) }
}
