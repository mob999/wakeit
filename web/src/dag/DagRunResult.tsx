import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { DagRunNode } from '../api'
import { RunSection } from '../runs/RunSection'
import { RunResult, RunResultActions, dagOutputs } from '../runs/RunResult'

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
      <RunResult
        result={value}
        raw={raw}
        toast={toast}
        outputs={outputs ?? []}
        selectedOutput={output?.node_id}
        onSelectOutput={setSelected}
        identity={`${runId}:${output?.node_id ?? ''}:${execution ?? runId}`}
        cwd={nodes.find((node) => node.id === output?.node_id)?.cwd}
        source={
          execution
            ? { runId: execution }
            : { runId, ...(output ? { nodeId: output.node_id } : {}) }
        }
      />
    </RunSection>
  )
}
