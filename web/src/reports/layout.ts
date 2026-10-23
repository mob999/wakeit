export type ReportWidths = { navigation: number; references: number }
export const defaultReportWidths: ReportWidths = { navigation: 200, references: 240 }
export const reportWidthLimits = {
  navigation: { min: 160, max: 400 },
  references: { min: 180, max: 480 },
}
export function reportWidths(
  room: number,
  navigation: boolean,
  references: boolean,
  preferred: ReportWidths,
) {
  const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value))
  const widths = {
    navigation: clamp(preferred.navigation, 160, 400),
    references: clamp(preferred.references, 180, 480),
  }
  const left = navigation && room >= 1080,
    right = references && room >= 740
  const budget = Math.max(0, room - 360)
  const occupied = (left ? widths.navigation : 0) + (right ? widths.references : 0)
  const excess = Math.max(0, occupied - budget)
  if (excess) {
    const leftSlack = left ? widths.navigation - 160 : 0
    const rightSlack = right ? widths.references - 180 : 0
    const slack = leftSlack + rightSlack
    if (slack) {
      widths.navigation -= (Math.min(excess, slack) * leftSlack) / slack
      widths.references -= (Math.min(excess, slack) * rightSlack) / slack
    }
  }
  return {
    widths,
    max: {
      navigation: Math.min(400, Math.max(160, budget - (right ? widths.references : 0))),
      references: Math.min(480, Math.max(180, budget - (left ? widths.navigation : 0))),
    },
  }
}
