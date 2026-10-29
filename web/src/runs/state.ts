export const terminal = (state: string) =>
  ['succeeded', 'warning', 'failed', 'cancelled', 'timed_out', 'skipped', 'interrupted'].includes(
    state,
  )
