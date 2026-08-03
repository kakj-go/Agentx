import { AlertTriangle, Clock3, GitBranch, ScrollText } from 'lucide-react'

import type { NodeExecution, Trace } from '../../shared/api/types'
import { StatusBadge } from '../../shared/components/status-badge'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../../shared/ui/tabs'
import { executionStatus } from './execution-format'
import { ExecutionJsonView } from './execution-json-view'

type NodePanelProps = {
  node?: NodeExecution
  trace?: Trace
  onDownloadArtifact: (id: string) => void
}

export function ExecutionNodePanel({ node, trace, onDownloadArtifact }: NodePanelProps) {
  if (!node) return <div className="grid min-h-[520px] place-items-center px-6 text-center text-sm text-muted-foreground">选择左侧节点查看运行数据</div>
  const logs = trace?.events.filter((event) => event.nodeExecutionId === node.id || event.nodeId === node.nodeId) ?? []
  return <section aria-label="节点运行数据" className="min-w-0 bg-surface">
    <header className="flex min-h-16 flex-wrap items-center gap-3 border-b border-border px-4 py-3">
      <div className="min-w-0 flex-1"><h2 className="truncate text-sm font-semibold">{node.nodeName}</h2><p className="mt-1 truncate text-[10px] text-muted-foreground">{node.nodeType}@{node.nodeVersion} · Run {node.runIndex} · {node.capability}</p></div>
      <StatusBadge status={executionStatus(node.status)} />
    </header>
    {node.errorMessage && <div className="flex gap-2 border-b border-danger/25 bg-danger/10 px-4 py-3 text-xs text-danger"><AlertTriangle className="mt-0.5 size-4 shrink-0" /><span><strong>{node.errorCode}</strong><br />{node.errorMessage}</span></div>}
    <Tabs defaultValue="output">
      <TabsList className="border-b border-border px-4">
        <TabsTrigger value="input">Input</TabsTrigger>
        <TabsTrigger value="output">Output</TabsTrigger>
        <TabsTrigger value="lineage">Lineage <span className="ml-1 text-[10px]">{node.lineage.length}</span></TabsTrigger>
        <TabsTrigger value="attempts">Attempts <span className="ml-1 text-[10px]">{node.attempts.length}</span></TabsTrigger>
        <TabsTrigger value="logs">Logs <span className="ml-1 text-[10px]">{logs.length}</span></TabsTrigger>
      </TabsList>
      <TabsContent value="input"><ExecutionJsonView emptyLabel="该节点没有输入" onDownloadArtifact={onDownloadArtifact} value={node.input} /></TabsContent>
      <TabsContent value="output"><ExecutionJsonView emptyLabel="该节点尚无输出" onDownloadArtifact={onDownloadArtifact} value={node.output} /></TabsContent>
      <TabsContent className="max-h-[54vh] min-h-64 overflow-auto" value="lineage">
        {node.lineage.length ? <div className="divide-y divide-border">{node.lineage.map((source, index) => <div className="grid grid-cols-[28px_minmax(0,1fr)] gap-3 px-4 py-3" key={`${source.deliveryId}:${index}`}><span className="grid size-7 place-items-center rounded-md bg-primary/10 text-primary"><GitBranch className="size-3.5" /></span><div className="min-w-0"><p className="truncate font-mono text-[11px]">{source.sourceNodeExecutionId}</p><p className="mt-1 text-[10px] text-muted-foreground">Run {source.sourceRunIndex} · Output {source.sourceOutputIndex} · Item {source.sourceItemIndex} → Item {source.targetItemIndex}</p></div></div>)}</div> : <PanelEmpty icon={GitBranch} label="没有 Lineage 记录" />}
      </TabsContent>
      <TabsContent className="max-h-[54vh] min-h-64 overflow-auto" value="attempts">
        {node.attempts.length ? <div className="divide-y divide-border">{node.attempts.map((attempt) => <div className="px-4 py-3" key={attempt.id}><div className="flex items-center justify-between gap-3"><div className="flex items-center gap-2"><Clock3 className="size-3.5 text-muted-foreground" /><strong className="text-xs">Attempt {attempt.attemptNumber}</strong></div><StatusBadge status={executionStatus(attempt.status)} /></div><dl className="mt-2 grid grid-cols-2 gap-x-4 gap-y-1 text-[10px] text-muted-foreground"><dt>Worker</dt><dd className="truncate text-right">{attempt.workerInstanceId ?? '—'}</dd><dt>Deadline</dt><dd className="text-right">{formatTime(attempt.deadlineAt)}</dd><dt>Started</dt><dd className="text-right">{formatTime(attempt.startedAt)}</dd></dl>{attempt.errorMessage && <p className="mt-2 text-[11px] text-danger">{attempt.errorCode}: {attempt.errorMessage}</p>}</div>)}</div> : <PanelEmpty icon={Clock3} label="没有 Attempt 记录" />}
      </TabsContent>
      <TabsContent className="max-h-[54vh] min-h-64 overflow-auto" value="logs">
        {logs.length ? <div className="divide-y divide-border">{logs.map((event) => <div className="px-4 py-3" key={event.eventId}><div className="flex items-center justify-between gap-4 text-[11px]"><strong>{event.eventType}</strong><span className="text-muted-foreground">{formatTime(event.eventTime)}</span></div><pre className="mt-2 whitespace-pre-wrap break-all font-mono text-[10px] leading-5 text-muted-foreground">{event.errorMessage ?? JSON.stringify(event.attributes, null, 2)}</pre></div>)}</div> : <PanelEmpty icon={ScrollText} label="没有节点日志" />}
      </TabsContent>
    </Tabs>
  </section>
}

function formatTime(value?: string | null) {
  return value ? new Date(value).toLocaleString() : '—'
}

function PanelEmpty({ icon: Icon, label }: { icon: typeof GitBranch; label: string }) {
  return <div className="grid min-h-64 place-items-center text-xs text-muted-foreground"><div className="text-center"><Icon className="mx-auto mb-3 size-5" />{label}</div></div>
}
