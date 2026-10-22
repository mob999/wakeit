import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { MarkdownContent } from './MarkdownContent'
import { CopyValue } from './CopyValue'
import type { FileSource } from '../files/links'
import { CodeContent, isJsonCode } from './CodeContent'
import { ReportReader, type ReportOutput } from '../reports/ReportReader'
export function resultData(result: unknown) {
  const truncated =
    result !== null &&
    typeof result === 'object' &&
    'truncated' in result &&
    result.truncated === true &&
    'preview' in result &&
    typeof result.preview === 'string'
  return {
    truncated,
    text: truncated ? (result as { preview: string }).preview : result,
    source: typeof result === 'string' ? result : JSON.stringify(result, null, 2),
    empty: result == null || result === '',
  }
}
export function RunResultActions({
  result,
  raw,
  onRaw,
  toast,
}: {
  result: unknown
  raw: boolean
  onRaw: (raw: boolean) => void
  toast: (message: string) => void
}) {
  useTranslation()
  const { text, source, empty } = resultData(result)
  return (
    <>
      {(typeof text === 'string' || dagOutputs(result)) && !empty && (
        <div className="result-mode">
          <button aria-pressed={!raw} className={!raw ? 'active' : ''} onClick={() => onRaw(false)}>
            {tr('阅读')}
          </button>
          <button aria-pressed={raw} className={raw ? 'active' : ''} onClick={() => onRaw(true)}>
            {tr('原文')}
          </button>
        </div>
      )}
      {!empty && <CopyValue value={source ?? ''} toast={toast} />}
    </>
  )
}
export function RunResult({
  result,
  raw,
  source,
  cwd,
  outputs = [],
  selectedOutput,
  onSelectOutput,
  identity,
}: {
  result: unknown
  raw: boolean
  toast?: (s: string) => void
  source?: FileSource
  cwd?: string
  outputs?: ReportOutput[]
  selectedOutput?: string
  onSelectOutput?: (id: string) => void
  identity?: string
}) {
  useTranslation()
  const { truncated, text, source: rawSource, empty } = resultData(result)
  if (empty && !outputs.length) return null
  return (
    <div className="run-result-panel">
      <ReportReader
        identity={
          identity ?? `${source?.runId ?? ''}:${source?.nodeId ?? ''}:${source?.runId ?? ''}`
        }
        indexKey={rawSource ?? ''}
        markdown={!raw && typeof text === 'string' && !empty}
        source={source}
        cwd={cwd}
        outputs={outputs}
        selectedOutput={selectedOutput}
        onSelectOutput={onSelectOutput}
        notice={truncated && <p className="quiet">{tr('结果已截断')}</p>}
      >
        {empty ? (
          <p className="quiet">{tr('未返回结果')}</p>
        ) : raw || typeof text !== 'string' ? (
          <CodeContent
            text={rawSource ?? ''}
            json={isJsonCode(result)}
            className="result result-raw"
          />
        ) : (
          <article className="markdown-result result">
            <MarkdownContent text={text} source={source} />
          </article>
        )}
      </ReportReader>
    </div>
  )
}

export type DagOutput = {
  node_id: string
  name: string
  state: string
  reason?: string | null
  result: unknown
}
export function dagOutputs(result: unknown): DagOutput[] | null {
  if (
    !result ||
    typeof result !== 'object' ||
    !('type' in result) ||
    result.type !== 'dag' ||
    !('outputs' in result) ||
    !Array.isArray(result.outputs)
  )
    return null
  return result.outputs.every(
    (o) =>
      o &&
      typeof o.node_id === 'string' &&
      typeof o.name === 'string' &&
      typeof o.state === 'string',
  )
    ? result.outputs
    : null
}
