export const recordingReasons = [
  'content_shortened',
  'event_limit',
  'event_unsupported',
  'event_parse_failed',
  'event_too_large',
  'content_limit',
  'content_expired',
  'unspecified',
] as const
export type RecordingReason = (typeof recordingReasons)[number]

export function recordingReason(data: {
  status?: unknown
  reason?: unknown
}): RecordingReason | null {
  if (data.status === 'expired') return 'content_expired'
  if (data.status !== 'incomplete' && data.status !== 'truncated') return null
  return recordingReasons.includes(data.reason as RecordingReason)
    ? (data.reason as RecordingReason)
    : 'unspecified'
}
