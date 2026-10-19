import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api, type Runner } from '../api'
import { runnerName } from './identity'
import { outputs } from './model'
import { type TaskConfig, type Agent } from '../api'
import { t } from '../i18n'
import { DagGraph } from './DagGraph'
import { AgentLogo } from '../AgentLogo'
export function DagTaskOverview({ config, agents }: { config: TaskConfig; agents?: Agent[] }) {
  const runners = useQuery<Runner[]>({ queryKey: ['runners'], queryFn: () => api('/runners') })
  const [selected, setSelected] = useState(config.dag!.nodes[0]?.id)
  const node = config.dag!.nodes.find((n) => n.id === selected)
  const agent =
    node &&
    agents?.find(
      (a) => a.id === node.agent && (a.runner === node.runner || a.runner_id === node.runner),
    )
  return (
    <section className="content-card">
      <DagGraph
        compact
        runners={runners.data}
        outputIds={outputs(config.dag!)}
        nodes={config.dag!.nodes}
        agents={agents}
        selected={selected}
        onSelect={setSelected}
      />
      {node && (
        <div className="dag-node-readonly">
          <dl className="properties">
            <dt>{t('dag.field.name')}</dt>
            <dd>{node.name}</dd>
            <dt>{t('Runner')}</dt>
            <dd>{runnerName(node, runners.data ?? [], agents ?? [])}</dd>
            <dt>{t('Agent')}</dt>
            <dd>
              <span className="run-agent">
                <AgentLogo name={agent?.name ?? node.agent} logo={agent?.logo} />
                {agent?.name ?? node.agent}
              </span>
            </dd>
            <dt>{t('工作目录')}</dt>
            <dd className="mono wrap">{node.cwd}</dd>
            <dt>{t('参数')}</dt>
            <dd className="mono wrap">{JSON.stringify(node.parameters ?? {}, null, 2)}</dd>
            <dt>{t('会话')}</dt>
            <dd>{node.session?.id ?? t('每次新建')}</dd>
            <dt>{t('执行超时')}</dt>
            <dd>{node.timeout}</dd>
            <dt>{t('指令')}</dt>
            <dd>
              <pre className="task-prompt">{node.prompt}</pre>
            </dd>
          </dl>
        </div>
      )}
    </section>
  )
}
