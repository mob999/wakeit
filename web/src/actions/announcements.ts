export const actionStatusEvent = 'wakeit-action-status'
export function announceAction(message: string) {
  window.dispatchEvent(new CustomEvent(actionStatusEvent, { detail: message }))
}
