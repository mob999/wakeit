import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { DagRunNode } from '../api'
import { ChoicePicker } from '../ChoicePicker'
import { Status } from '../Status'
import { RunReason } from '../runs/RunReason'
import { RunSection } from '../runs/RunSection'
import { RunResult, RunResultActions, dagOutputs, resultData } from '../runs/RunResult'
import { shortId } from './identity'

export function DagRunResult({
  runId,
  result,
  nodes,
  open,
  onOpenChange,
  hidden,
  toast,
}: {
  runId: string
  result: unknown
  nodes: DagRunNode[]
  open: boolean
  onOpenChange: (value: boolean) => void
  hidden: boolean
  toast: (message: string) => void
}) {
  const { t } = useTranslation()
  const [selected, setSelected] = useState(() => dagOutputs(result)?.[0]?.node_id)
  const [modes, setModes] = useState<Record<string, boolean>>({})
  const outputs = dagOutputs(result)
  const output = outputs?.find((item) => item.node_id === selected) ?? outputs?.[0]
  useEffect(() => {
    if (output && output.node_id !== selected) setSelected(output.node_id)
  }, [output?.node_id, selected])
  const key = output?.node_id ?? ''
  const value = outputs ? output?.result : result
  const raw = modes[key] ?? false
  const execution = nodes.find((node) => node.id === output?.node_id)?.execution_id
  return (
    <RunSection
      title={t('结果')}
      open={open}
      onChange={onOpenChange}
      hidden={hidden}
      actions={
        <RunResultActions
          result={value}
          raw={raw}
          onRaw={(mode) => setModes((old) => ({ ...old, [key]: mode }))}
          toast={toast}
        />
      }
    >
      {output && (
        <div className="dag-result-context">
          {outputs!.length > 1 ? (
            <ChoicePicker
              label={t('dag.selectOutput')}
              searchLabel={t('dag.searchOutputs')}
              popoverClassName="dag-node-picker"
              searchable
              value={output.node_id}
              options={outputs!.map((item) => ({
                value: item.node_id,
                label: item.name,
                title: item.name,
                keywords: item.node_id,
                description: shortId(item.node_id),
                status: <Status state={item.state} />,
              }))}
              onChange={setSelected}
            />
          ) : (
            <strong>{output.name}</strong>
          )}
          <RunReason state={output.state} reason={output.reason}>
            <Status state={output.state} />
          </RunReason>
        </div>
      )}
      {resultData(value).empty ? (
        <p className="quiet dag-node-empty">{t('未返回结果')}</p>
      ) : (
        <RunResult
          key={key}
          result={value}
          raw={raw}
          toast={toast}
          source={
            execution
              ? { runId: execution }
              : { runId, ...(output ? { nodeId: output.node_id } : {}) }
          }
        />
      )}
    </RunSection>
  )
}
