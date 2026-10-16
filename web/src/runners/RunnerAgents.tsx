import { t as tr, parameterLabel } from '../i18n'
import { useTranslation } from 'react-i18next'
import { ChevronDown, ChevronRight, Plus } from 'lucide-react'
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
  useTranslation()
  const agents = runner.agents.filter((a) =>
    agentMatches({ ...a, runner: runner.name, runner_id: runner.id }, filters),
  )
  return (
    <DetailBody>
      {!agents.length ? (
        <p className="quiet card-state content-card">
          {runner.agents.length ? tr('无匹配 Agent') : tr('暂无 Agent')}
        </p>
      ) : (
        <div className="runner-agents-table content-card">
          <div className="runner-agent-columns agent-table-head table-head">
            <span>{tr('Agent')}</span>
            <span>{tr('版本')}</span>
            <span>{tr('检测状态')}</span>
            <span className="agent-actions-heading">{tr('操作')}</span>
          </div>
          {agents.map((agent) => (
            <div className="runner-agent-item" key={agent.id}>
              <div className="runner-agent-columns">
                <button
                  className="agent-expand"
                  aria-label={`${expanded === agent.id ? tr('收起') : tr('展开')} ${agent.name}`}
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
                  {agent.available ? tr('已检测到') : tr('未检测到')}
                </span>
                <div className="agent-row-actions">
                  {onCreate && (
                    <IconButton
                      className="agent-create-button"
                      label={tr('创建任务')}
                      disabled={runner.revoked || !createReady}
                      onClick={() => onCreate(agent.id)}
                    >
                      <Plus />
                    </IconButton>
                  )}
                </div>
              </div>
              {expanded === agent.id && (
                <div id={`agent-detail-${agent.id}`} className="agent-details">
                  <div className="content-card agent-capabilities">
                    <table className="agent-detail-table">
                      <thead className="table-head">
                        <tr>
                          <th scope="col">{tr('执行方式')}</th>
                          <th scope="col">{tr('取消')}</th>
                          <th scope="col">{tr('模型选择')}</th>
                        </tr>
                      </thead>
                      <tbody>
                        <tr>
                          <td>{agent.mode === 'sync' ? tr('同步') : tr('异步')}</td>
                          <td>{agent.cancellable ? tr('支持') : tr('不支持')}</td>
                          <td>{agent.models_supported ? tr('支持查询') : tr('手填')}</td>
                        </tr>
                      </tbody>
                    </table>
                  </div>
                  {!!Object.keys(agent.parameters).length && (
                    <div className="content-card agent-parameter-table">
                      <table className="agent-detail-table">
                        <thead className="table-head">
                          <tr>
                            <th scope="col">{tr('参数')}</th>
                            <th scope="col">{tr('默认值')}</th>
                            <th scope="col">{tr('可选值')}</th>
                          </tr>
                        </thead>
                        <tbody>
                          {Object.entries(agent.parameters).map(([id, spec]) => (
                            <tr key={id}>
                              <td>
                                {parameterLabel(spec.label || id)}
                                <small className="mono quiet">{id}</small>
                              </td>
                              <td className="mono">
                                {spec.default ?? (id === 'model' ? tr('Agent 默认') : '—')}
                              </td>
                              <td className="mono quiet">
                                {spec.enum.length ? spec.enum.join(', ') : '—'}
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
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
