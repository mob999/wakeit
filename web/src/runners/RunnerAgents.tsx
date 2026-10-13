import { ChevronDown, ChevronRight, ListPlus } from 'lucide-react'
import { type Runner } from '../api'
import { agentMatches, type Filters } from '../Search'
import { DetailBody } from '../Workbench'
import { AgentLogo } from '../AgentLogo'
import { IconButton } from '../ui'
export function RunnerAgents({
  runner,
  filters,
  expanded,
  onExpand,
  onCreate,
  createReady,
}: {
  runner: Runner
  filters: Filters
  expanded?: string
  onExpand: (id?: string) => void
  onCreate?: (id: string) => void
  createReady: boolean
}) {
  const agents = runner.agents.filter((a) =>
    agentMatches({ ...a, runner: runner.name, runner_id: runner.id }, filters),
  )
  return (
    <DetailBody>
      {!agents.length ? (
        <p className="quiet card-state content-card">
          {runner.agents.length ? '无匹配 Agent' : '暂无 Agent'}
        </p>
      ) : (
        <div className="runner-agents-table content-card">
          <div className="runner-agent-columns agent-table-head table-head">
            <span>Agent</span>
            <span>版本</span>
            <span>检测状态</span>
            <span className="agent-actions-heading">操作</span>
          </div>
          {agents.map((agent) => (
            <div className="runner-agent-item" key={agent.id}>
              <div className="runner-agent-columns">
                <button
                  className="agent-expand"
                  aria-label={`${expanded === agent.id ? '收起' : '展开'} ${agent.name}`}
                  aria-expanded={expanded === agent.id}
                  aria-controls={`agent-detail-${agent.id}`}
                  onClick={() => onExpand(expanded === agent.id ? undefined : agent.id)}
                >
                  {expanded === agent.id ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
                  <AgentLogo name={agent.name} logo={agent.logo} />
                  <span className="agent-display-name">{agent.name}</span>
                </button>
                <span className="mono quiet agent-version">{agent.version || '—'}</span>
                <span className={`status ${agent.available ? 'succeeded' : 'failed'}`}>
                  <i />
                  {agent.available ? '已检测到' : '未检测到'}
                </span>
                <div className="agent-row-actions">
                  {onCreate && (
                    <IconButton
                      className="agent-create-button"
                      label="创建任务"
                      disabled={runner.revoked || !createReady}
                      onClick={() => onCreate(agent.id)}
                    >
                      <ListPlus />
                    </IconButton>
                  )}
                </div>
              </div>
              {expanded === agent.id && (
                <div id={`agent-detail-${agent.id}`} className="agent-details">
                  <dl className="properties">
                    <dt>执行方式</dt>
                    <dd>{agent.mode === 'sync' ? '同步' : '异步'}</dd>
                    <dt>取消</dt>
                    <dd>{agent.cancellable ? '支持' : '不支持'}</dd>
                    <dt>模型选择</dt>
                    <dd>{agent.models_supported ? '支持查询' : '手填'}</dd>
                  </dl>
                  {!!Object.keys(agent.parameters).length && (
                    <div className="agent-parameter-table">
                      <div className="agent-parameter-row quiet">
                        <span>参数</span>
                        <span>默认值</span>
                        <span>可选值</span>
                      </div>
                      {Object.entries(agent.parameters).map(([id, spec]) => (
                        <div className="agent-parameter-row" key={id}>
                          <span>
                            {spec.label || id}
                            <small className="mono quiet">{id}</small>
                          </span>
                          <span className="mono">
                            {spec.default ?? (id === 'model' ? 'Agent 默认' : '—')}
                          </span>
                          <span className="mono quiet">
                            {spec.enum.length ? spec.enum.join(' · ') : '—'}
                          </span>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </DetailBody>
  )
}
