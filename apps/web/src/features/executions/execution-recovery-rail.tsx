import { CheckCircle2, Clock3, Download, ExternalLink, GitFork, History, ShieldAlert, TimerReset } from 'lucide-react'
import type { ReactNode } from 'react'
import { Link } from 'react-router-dom'

import type { Approval, Checkpoint, ExecutionWait, NodeExecution, Trace } from '../../shared/api/types'
import { Button } from '../../shared/ui/button'

type RecoveryRailProps = {
  trace?: Trace
  checkpoints: Checkpoint[]
  waits: ExecutionWait[]
  approvals: Approval[]
  nodes: NodeExecution[]
  canConfirm: boolean
  onConfirm: (node: NodeExecution) => void
  onDownloadArtifact: (artifactId: string) => void
}

export function ExecutionRecoveryRail({ trace, checkpoints, waits, approvals, nodes, canConfirm, onConfirm, onDownloadArtifact }: RecoveryRailProps) {
  const events = trace?.events.slice().reverse().slice(0, 16) ?? []
  const confirmations = nodes.filter((node) => node.sideEffectLevel === 'irreversible' && node.status === 'waiting')
  return <aside aria-label="恢复与事件" className="min-w-0 border-l border-border bg-surface max-xl:border-l-0 max-xl:border-t">
    <div className="flex h-12 items-center border-b border-border px-4"><strong className="text-xs">Timeline & recovery</strong></div>
    <div className="max-h-[calc(100vh-250px)] overflow-auto max-xl:max-h-none">
      {waits.length > 0 && <RailSection icon={TimerReset} title="Wait">
        {waits.map((wait) => <div className="border-b border-border/70 py-2.5 last:border-0" key={wait.id}><div className="flex items-center justify-between gap-2"><strong className="text-[11px]">{wait.waitKind}</strong><span className="text-[10px] text-warning">{wait.status}</span></div><p className="mt-1 text-[10px] text-muted-foreground">Wake {formatTime(wait.wakeAt)}<br />Timeout {formatTime(wait.timeoutAt)}</p>{wait.resumeUrl && <p className="mt-1.5 truncate font-mono text-[9px] text-primary" title={wait.resumeUrl}>{wait.resumeUrl}</p>}</div>)}
      </RailSection>}
      {approvals.length > 0 && <RailSection icon={CheckCircle2} title="Approval">
        {approvals.map((approval) => <div className="py-2.5" key={approval.id}><div className="flex items-start justify-between gap-2"><div className="min-w-0"><strong className="block truncate text-[11px]">{approval.title}</strong><p className="mt-1 text-[10px] text-muted-foreground">{approval.status} · {approval.resumeStatus}</p></div><Button asChild aria-label="打开审批" size="icon" variant="ghost"><Link to={`/approvals/${approval.id}`}><ExternalLink className="size-3.5" /></Link></Button></div></div>)}
      </RailSection>}
      {confirmations.length > 0 && <RailSection icon={ShieldAlert} title="Side effect confirmation">
        {confirmations.map((node) => <div className="py-2.5" key={node.id}><strong className="block truncate text-[11px]">{node.nodeName}</strong><p className="my-2 text-[10px] leading-4 text-muted-foreground">不可逆节点正在等待恢复决策。</p><Button disabled={!canConfirm} onClick={() => onConfirm(node)} size="sm" variant="secondary">处理</Button></div>)}
      </RailSection>}
      <RailSection icon={GitFork} title={`Checkpoints · ${checkpoints.length}`}>
        {checkpoints.slice().reverse().slice(0, 8).map((checkpoint) => <div className="border-b border-border/70 py-2.5 last:border-0" key={checkpoint.id}><div className="flex items-center justify-between gap-2"><strong className="text-[11px]">#{checkpoint.sequenceNumber} {checkpoint.checkpointType}</strong><span className="text-[9px] text-muted-foreground">{formatTime(checkpoint.createdAt)}</span></div><p className="mt-1 truncate font-mono text-[9px] text-muted-foreground" title={checkpoint.stateHash}>{checkpoint.stateHash}</p><p className="mt-1 text-[9px] text-muted-foreground">{checkpoint.activationCount} activations · {checkpoint.deliveryCount} deliveries</p></div>)}
        {!checkpoints.length && <p className="py-3 text-[10px] text-muted-foreground">尚无 Checkpoint</p>}
      </RailSection>
      <RailSection icon={History} title="Events">
        {events.map((event) => <div className="relative border-l border-border pb-3 pl-4 last:pb-0" key={event.eventId}><span className="absolute -left-1 top-1 size-2 rounded-full bg-muted-foreground ring-2 ring-surface" /><div className="flex items-start justify-between gap-2"><strong className="text-[10px]">{event.eventType}</strong><span className="shrink-0 text-[9px] text-muted-foreground">{formatTime(event.eventTime)}</span></div><p className="mt-1 truncate text-[9px] text-muted-foreground">{event.nodeId ?? event.status}</p>{event.contentRef && <Button className="mt-2" onClick={() => onDownloadArtifact(event.contentRef!)} size="sm" variant="secondary"><Download className="size-3.5" />下载</Button>}</div>)}
        {!events.length && <p className="py-3 text-[10px] text-muted-foreground">尚无 Timeline 事件</p>}
      </RailSection>
    </div>
  </aside>
}

function RailSection({ icon: Icon, title, children }: { icon: typeof Clock3; title: string; children: ReactNode }) {
  return <section className="border-b border-border px-4 py-3"><h3 className="flex items-center gap-2 text-[10px] font-semibold uppercase text-muted-foreground"><Icon className="size-3.5" />{title}</h3><div className="mt-2">{children}</div></section>
}

function formatTime(value?: string | null) {
  return value ? new Date(value).toLocaleString() : '—'
}
