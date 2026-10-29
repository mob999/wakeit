import { useCopy } from '../actions/useCopy'
import { ActionIcon } from '../actions/ActionIcon'
import { useContext, useEffect, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate } from '@tanstack/react-router'
import { Monitor, Clock } from '../components/icons'
import { t } from '../i18n'
import { api } from '../api/client'
import { type MomentEntry, type Run, type Runner } from '../api/types'
import { Button, IconButton, TooltipTarget } from '../components/ui'
import { Status } from '../components/Status'
import { AgentLogo } from '../components/AgentLogo'
import { MarkdownContent } from '../markdown/MarkdownContent'
import { CodeContent, isJsonCode } from '../markdown/CodeContent'
import { dagOutputs, resultData } from '../runs/RunResult'
import { RunReason } from '../runs/RunReason'
import { ResumeRun } from '../sessions/ResumeRun'
import { duration, retriedNodes } from './reportModel'
import { ReportReader } from '../reports/ReportReader'
import { VisitContext, manual } from './MomentsPage'

export function MomentReader({
  entry,
  toast,
  onClose,
}: {
  entry: MomentEntry
  toast: (s: string) => void
  onClose: () => void
}) {
  useTranslation()
  const visit = useContext(VisitContext),
    client = useQueryClient()
  const [output, setOutput] = useState(visit.outputs.current.get(entry.id))
  const [copying, setCopying] = useState(false)
  const { copied, copy: copyOutput } = useCopy(toast, t('actions.copyOutput'))
  const navigate = useNavigate()
  const queryKey = ['moments', 'report', entry.id]
  const full = useQuery({
    queryKey,
    queryFn: ({ signal }) => api<Run>(`/runs/${entry.id}`, 'GET', undefined, undefined, signal),
    ...manual,
    refetchOnMount: true,
    gcTime: 60_000,
  })
  useEffect(() => {
    if (full.error) toast(full.error.message)
  }, [full.error, toast])
  const actualOutputs = dagOutputs(full.data?.result)
  const outputs = actualOutputs ?? entry.outputs ?? []
  const chosen = outputs.find((o) => o.node_id === output) ?? outputs[0]
  const nodeId = chosen?.node_id
  const activeNode = full.data?.nodes?.find((node) => node.id === nodeId)
  const runners = useQuery<Runner[]>({
    queryKey: ['runners'],
    queryFn: () => api('/runners'),
    enabled: entry.format === 'dag',
  })
  const device = runners.data?.find(
    (runner) => runner.id === activeNode?.runner || runner.name === activeNode?.runner,
  )
  const agent = device?.agents.find((agent) => agent.id === activeNode?.agent)
  const deviceName = device?.name ?? activeNode?.runner ?? entry.runner ?? '—'
  const agentName = agent?.name ?? activeNode?.agent ?? entry.agent ?? '—'
  const execution = activeNode?.execution_id
  const fileSource = execution
    ? { runId: execution }
    : { runId: entry.id, ...(nodeId ? { nodeId } : {}) }
  useEffect(() => {
    if (nodeId) visit.outputs.current.set(entry.id, nodeId)
  }, [entry.id, nodeId, visit.outputs])
  const actual = actualOutputs?.find((o) => o.node_id === nodeId)
  const preview = entry.outputs?.find((o) => o.node_id === nodeId)
  const isDag = entry.format === 'dag'
  const value = isDag
    ? actual
      ? actual.result
      : preview?.preview
    : full.data
      ? full.data.result
      : entry.preview
  const data = resultData(value)
  const text = typeof data.text === 'string' ? data.text : (data.source ?? '')
  const json = isDag
    ? actual
      ? typeof data.text !== 'string'
      : preview?.format === 'json'
    : full.data
      ? typeof data.text !== 'string'
      : entry.format === 'json'
  const incomplete = !full.data
  const truncated = full.data
    ? data.truncated
    : data.truncated || (isDag ? preview?.truncated : entry.truncated)
  function choose(id: string) {
    setOutput(id)
    visit.outputs.current.set(entry.id, id)
  }
  async function copy() {
    setCopying(true)
    try {
      const current =
        full.data ??
        (await client.fetchQuery<Run>({
          queryKey,
          queryFn: () => api<Run>(`/runs/${entry.id}`),
          staleTime: Infinity,
        }))
      if (!current) throw new Error(t('未返回结果'))
      const result = isDag
        ? dagOutputs(current.result)?.find((o) => o.node_id === nodeId)?.result
        : current.result
      await copyOutput(resultData(result).source ?? '')
    } catch (e) {
      toast((e as Error).message)
    } finally {
      setCopying(false)
    }
  }
  const elapsed = duration(full.data?.started_at, full.data?.finished_at)
  const retries = retriedNodes(full.data)
  const resumable = isDag
    ? !!full.data?.nodes?.find((n) => n.id === nodeId)?.continuation_available
    : entry.continuation_available
  return (
    <section className="content-card moment-reader">
      <header className="card-header">
        <Button className="moment-back" onClick={onClose}>
          <ActionIcon name="back" />
          {t('moments.back')}
        </Button>
        <h3>
          <Link to="/runs" search={{ run: entry.id }}>
            {entry.name}
          </Link>
        </h3>
        <div className="moment-identity">
          <TooltipTarget label={deviceName}>
            <span>
              <Monitor size={12} />
              <span className="moment-device">{deviceName}</span>
            </span>
          </TooltipTarget>
          <TooltipTarget label={agentName}>
            <span>
              <AgentLogo name={agentName} logo={agent?.logo ?? entry.logo} />
              <span className="moment-agent-name">{agentName}</span>
            </span>
          </TooltipTarget>
        </div>

        <div className="moment-actions">
          {elapsed && (
            <TooltipTarget label={t('moments.elapsed')}>
              <span className="moment-elapsed">
                <Clock size={12} />
                {elapsed}
              </span>
            </TooltipTarget>
          )}
          <RunReason state={entry.state} reason={entry.reason}>
            <Status state={entry.state} />
          </RunReason>
          {full.error && (
            <IconButton
              label={t('actions.reloadResult')}
              loading={full.isFetching}
              onClick={() => full.refetch()}
            >
              <ActionIcon name="refresh" />
            </IconButton>
          )}
          <ResumeRun
            id={entry.id}
            available={resumable}
            initialNode={nodeId}
            strictNode={isDag}
            toast={toast}
          />
          {(!data.empty || isDag) && (
            <IconButton
              label={t('actions.copyOutput')}
              tooltip={copied ? t('已复制') : t('actions.copyOutput')}
              loading={copying}
              onClick={copy}
            >
              {copied ? <ActionIcon name="check" /> : <ActionIcon name="copy" />}
            </IconButton>
          )}

          <IconButton
            label={t('查看运行')}
            onClick={() => navigate({ to: '/runs', search: { run: entry.id } })}
          >
            <ActionIcon name="open" />
          </IconButton>
        </div>
      </header>
      <div className="moment-report-scroll">
        <ReportReader
          fill
          identity={`${entry.id}:${nodeId ?? ''}:${execution ?? entry.id}`}
          indexKey={text}
          markdown={!json && !data.empty}
          source={fileSource}
          cwd={full.data?.nodes?.find((node) => node.id === nodeId)?.cwd ?? full.data?.config.cwd}
          outputs={isDag ? outputs : []}
          selectedOutput={nodeId}
          onSelectOutput={choose}
          incomplete={!!incomplete}
          notice={
            <>
              {retries > 0 && (
                <p className="moment-retries quiet text-muted">
                  {t('moments.retried', { count: retries })}
                </p>
              )}
              {truncated && <p className="quiet text-muted moment-notice">{t('结果已截断')}</p>}
            </>
          }
        >
          {!data.empty ? (
            json ? (
              <CodeContent text={text} json={isJsonCode(value)} className="result-raw" />
            ) : (
              <article className="markdown-result">
                <MarkdownContent text={text} source={fileSource} />
              </article>
            )
          ) : incomplete ? (
            <p className="quiet text-muted">{t('moments.loadOutput')}</p>
          ) : (
            <p className="quiet text-muted">{t('未返回结果')}</p>
          )}
        </ReportReader>
      </div>
    </section>
  )
}
