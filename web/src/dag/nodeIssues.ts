import type { DagRunNode } from '../api'

export function nodeIssues(nodes: DagRunNode[], state: string) {
  const failed = nodes.filter(
    (node) => ['failed', 'timed_out', 'interrupted'].includes(node.state) && !node.retry_at,
  )
  const unknown = nodes.filter((node) => node.state === 'unknown')
  return {
    issues: state === 'unknown' ? [...unknown, ...failed] : [...failed, ...unknown],
    skipped: nodes.filter((node) => node.state === 'skipped'),
  }
}
