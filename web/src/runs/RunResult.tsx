import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { MarkdownContent } from './MarkdownContent'
import { DetailBody } from '../Workbench'
import { CopyValue } from './CopyValue'
import { Status } from '../Status'
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
  toast,
}: {
  result: unknown
  raw: boolean
  toast?: (s: string) => void
}) {
  useTranslation()
  const { truncated, text, source, empty } = resultData(result)
  if (empty) return null
  const outputs = dagOutputs(result)
  if (outputs && !raw)
    return (
      <div className="dag-results">
        {outputs.map((output) => (
          <section className="dag-result-group" key={output.node_id}>
            <header>
              <strong>{output.name}</strong>
              <Status state={output.state} />
              {toast && (
                <CopyValue
                  label={tr('复制结果')}
                  value={resultData(output.result).source ?? ''}
                  toast={toast}
                />
              )}
            </header>
            {output.reason && output.state !== 'succeeded' && (
              <p className="quiet dag-node-empty">{output.reason}</p>
            )}
            {resultData(output.result).empty ? (
              <p className="quiet dag-node-empty">{tr('未返回结果')}</p>
            ) : (
              <RunResult result={output.result} raw={false} />
            )}
          </section>
        ))}
      </div>
    )

  return (
    <div className="run-result-panel">
      <DetailBody>
        {truncated && <p className="quiet">{tr('结果已截断')}</p>}
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
