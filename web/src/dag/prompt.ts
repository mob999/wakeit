import type { NodeInstruction } from '../api'
const tokens = /\{\{\s*nodes\.([a-zA-Z0-9_-]+)\.result\s*\}\}/g
export function promptReferences(prompt: NodeInstruction): string[] {
  return typeof prompt === 'string'
    ? Array.from(prompt.matchAll(tokens), (match) => match[1])
    : prompt.parts.flatMap((part) => ('result' in part ? [part.result] : []))
}
// For display only. Editing and execution retain the original typed parts.
export function promptText(prompt?: NodeInstruction): string {
  if (!prompt) return ''
  return typeof prompt === 'string'
    ? prompt
    : prompt.parts
        .map((part) => ('text' in part ? part.text : '{{ nodes.' + part.result + '.result }}'))
        .join('')
}
export function promptBlank(prompt?: NodeInstruction): boolean {
  if (!prompt) return true
  return typeof prompt === 'string'
    ? !prompt.trim()
    : !prompt.parts.some((part) => 'result' in part || part.text.trim())
}
