// Paths belong to the Runner, never to the browser's operating system.
export function directorySeparator(path: string) {
  return path.includes('\\') || /^[A-Za-z]:/.test(path) ? '\\' : '/'
}
export function directoryPath(path: string | null) {
  if (!path) return ''
  return /[\\/]$/.test(path) ? path : path + directorySeparator(path)
}
export function directoryInput(value: string, base: string | null) {
  const absolute =
    value.startsWith('/') || value.startsWith('\\\\') || /^[A-Za-z]:[\\/]/.test(value)
  const path = absolute || !base ? value : directoryPath(base) + value
  const separator = directorySeparator(path)
  const index =
    separator === '\\'
      ? Math.max(path.lastIndexOf('\\'), path.lastIndexOf('/'))
      : path.lastIndexOf('/')
  if (index < 0) return { path: base, filter: value }
  return { path: path.slice(0, index + 1), filter: path.slice(index + 1) }
}
