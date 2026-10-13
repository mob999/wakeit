import { MarkdownContent } from './MarkdownContent'
import { DetailBody } from '../Workbench'
import { CopyValue } from './CopyValue'
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
  const { text, source, empty } = resultData(result)
  return (
    <>
      {typeof text === 'string' && !empty && (
        <div className="result-mode">
          <button aria-pressed={!raw} className={!raw ? 'active' : ''} onClick={() => onRaw(false)}>
            阅读
          </button>
          <button aria-pressed={raw} className={raw ? 'active' : ''} onClick={() => onRaw(true)}>
            原文
          </button>
        </div>
      )}
      {!empty && <CopyValue value={source ?? ''} toast={toast} />}
    </>
  )
}
export function RunResult({ result, raw }: { result: unknown; raw: boolean }) {
  const { truncated, text, source, empty } = resultData(result)
  if (empty) return null
  return (
    <div className="run-result-panel">
      <DetailBody>
        {truncated && <p className="quiet">结果已截断</p>}
        {raw || typeof text !== 'string' ? (
          <pre className="code result result-raw">{source}</pre>
        ) : (
          <article className="markdown-result result">
            <MarkdownContent text={text} />
          </article>
        )}
      </DetailBody>
    </div>
  )
}
