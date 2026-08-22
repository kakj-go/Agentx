import { CheckCircle2, Clock3, ExternalLink, GitFork, ShieldAlert, TimerReset } from 'lucide-react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import type { Approval, Checkpoint, ExecutionEvent, ExecutionWait, NodeExecution } from '../../shared/api/types'
import { useLocaleFormat } from '../../shared/lib/locale-format'
import { localizedValue } from '../../shared/lib/localized-value'
import { Button } from '../../shared/ui/button'

type RecoveryRailProps = {
  checkpoints: Checkpoint[]
  waits: ExecutionWait[]
  approvals: Approval[]
  nodes: NodeExecution[]
  events: ExecutionEvent[]
  canConfirm: boolean
  onConfirm: (node: NodeExecution) => void
}

export function ExecutionRecoveryRail({ checkpoints, waits, approvals, nodes, events, canConfirm, onConfirm }: RecoveryRailProps) {
  const { t } = useTranslation()
  const { formatDateTime } = useLocaleFormat()
  const confirmations = nodes.filter((node) => node.sideEffectLevel === 'irreversible' && node.status === 'waiting')
  return <aside aria-label={t('executions.recovery.ariaLabel')} className="min-w-0 bg-surface">
    <div className="flex h-12 items-center border-b border-border px-4"><strong className="text-xs">{t('executions.recovery.title')}</strong></div>
    <div className="max-h-[calc(100vh-250px)] overflow-auto max-xl:max-h-none">
      {waits.length > 0 && <RailSection icon={TimerReset} title={t('executions.recovery.wait')}>
        {waits.map((wait) => <div className="border-b border-border/70 py-2.5 last:border-0" key={wait.id}><div className="flex items-center justify-between gap-2"><strong className="text-[11px]">{wait.waitKind}</strong><span className="text-[10px] text-warning">{localizedValue(t, 'common', wait.status)}</span></div><p className="mt-1 text-[10px] text-muted-foreground">{t('executions.recovery.wake')} {formatDateTime(wait.wakeAt)}<br />{t('executions.recovery.timeout')} {formatDateTime(wait.timeoutAt)}</p>{wait.resumeUrl && <p className="mt-1.5 truncate font-mono text-[9px] text-primary" title={wait.resumeUrl}>{wait.resumeUrl}</p>}</div>)}
      </RailSection>}
      {approvals.length > 0 && <RailSection icon={CheckCircle2} title={t('executions.recovery.approval')}>
        {approvals.map((approval) => <div className="py-2.5" key={approval.id}><div className="flex items-start justify-between gap-2"><div className="min-w-0"><strong className="block truncate text-[11px]">{approval.title}</strong><p className="mt-1 text-[10px] text-muted-foreground">{localizedValue(t, 'approvals.statuses', approval.status)} · {localizedValue(t, 'approvals.resumeStatuses', approval.resumeStatus)}</p></div><Button asChild aria-label={t('executions.recovery.openApproval')} size="icon" variant="ghost"><Link to={`/approvals/${approval.id}`}><ExternalLink className="size-3.5" /></Link></Button></div></div>)}
      </RailSection>}
      {confirmations.length > 0 && <RailSection icon={ShieldAlert} title={t('executions.recovery.sideEffectConfirmation')}>
        {confirmations.map((node) => <div className="py-2.5" key={node.id}><strong className="block truncate text-[11px]">{node.nodeName}</strong><p className="my-2 text-[10px] leading-4 text-muted-foreground">{t('executions.recovery.irreversibleWaiting')}</p><Button disabled={!canConfirm} onClick={() => onConfirm(node)} size="sm" variant="secondary">{t('executions.recovery.handle')}</Button></div>)}
      </RailSection>}
      <RailSection icon={GitFork} title={t('executions.recovery.checkpoints', { count: checkpoints.length })}>
        {checkpoints.slice().reverse().slice(0, 8).map((checkpoint) => <div className="border-b border-border/70 py-2.5 last:border-0" key={checkpoint.id}><div className="flex items-center justify-between gap-2"><strong className="text-[11px]">#{checkpoint.sequenceNumber} {checkpoint.checkpointType}</strong><span className="text-[9px] text-muted-foreground">{formatDateTime(checkpoint.createdAt)}</span></div><p className="mt-1 truncate font-mono text-[9px] text-muted-foreground" title={checkpoint.stateHash}>{checkpoint.stateHash}</p><p className="mt-1 text-[9px] text-muted-foreground">{t('executions.recovery.activationsAndDeliveries', { activations: checkpoint.activationCount, deliveries: checkpoint.deliveryCount })}</p></div>)}
        {!checkpoints.length && <p className="py-3 text-[10px] text-muted-foreground">{t('executions.recovery.noCheckpoints')}</p>}
      </RailSection>
      <RailSection icon={Clock3} title={t('executions.recovery.events')}>
        {events.slice().reverse().slice(0, 100).map((event) => <div className="grid grid-cols-[70px_minmax(0,1fr)_auto] gap-3 border-b border-border/70 py-2.5 text-[10px] last:border-0" key={event.sequence}><span className="font-mono text-muted-foreground">#{event.sequence}</span><span className="min-w-0"><strong className="block truncate text-foreground">{event.eventType}</strong><span className="mt-1 block truncate font-mono text-[9px] text-muted-foreground">{JSON.stringify(event.summary)}</span></span><span className="text-muted-foreground">{formatDateTime(event.occurredAt)}</span></div>)}
        {!events.length && <p className="py-3 text-[10px] text-muted-foreground">{t('executions.recovery.noEvents')}</p>}
      </RailSection>
    </div>
  </aside>
}

function RailSection({ icon: Icon, title, children }: { icon: typeof Clock3; title: string; children: ReactNode }) {
  return <section className="border-b border-border px-4 py-3"><h3 className="flex items-center gap-2 text-[10px] font-semibold uppercase text-muted-foreground"><Icon className="size-3.5" />{title}</h3><div className="mt-2">{children}</div></section>
}
