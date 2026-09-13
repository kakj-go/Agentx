import { Check, ChevronDown, Play } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { Execution, NodeExecution } from '../../shared/api/types'
import { Badge } from '../../shared/ui/badge'
import { cn } from '../../shared/lib/cn'
import { TraceNodeCard } from './trace-node-card'
import { TraceSemanticValue } from './trace-semantic-value'
import { statusTone } from './trace-model'

export function TraceNodeView({ execution, nodes, onNodeSelect, onDownloadArtifact }: { execution: Execution; nodes: NodeExecution[]; onNodeSelect?: (node: NodeExecution) => void; onDownloadArtifact?: (artifactId: string) => void }) {
  return <div className="h-full min-h-0 overflow-auto p-5" data-testid="trace-node-view"><div className="relative mx-auto max-w-5xl space-y-3 before:absolute before:bottom-7 before:left-6 before:top-7 before:w-px before:bg-border">
    <BoundaryCard icon={<Play className="size-4" />} kind="start" status={execution.status === 'queued' ? 'queued' : 'succeeded'} value={execution.input} />
    {nodes.map((node, index) => <TraceNodeCard defaultExpanded={nodes.length <= 3 && index === nodes.length - 1} executionId={execution.id} key={node.id} node={node} onDownloadArtifact={onDownloadArtifact} onNodeSelect={onNodeSelect} />)}
    <BoundaryCard icon={<Check className="size-4" />} kind="end" status={execution.status} value={execution.output} />
  </div></div>
}

function BoundaryCard({ kind, value, status, icon }: { kind: 'start' | 'end'; value: unknown; status: string; icon: React.ReactNode }) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(kind === 'end')
  return <article className={cn('relative ml-14 rounded-xl border border-border bg-surface', expanded && 'border-primary/25')} data-testid={`trace-${kind}-boundary`}>
    <div className="absolute -left-14 top-3.5 flex w-12 justify-center"><span className={cn('grid size-9 place-items-center rounded-lg border-4 border-surface shadow-[0_0_0_1px_var(--color-border)]', kind === 'start' ? 'bg-success/10 text-success' : 'bg-violet-500/10 text-violet-500')}>{icon}</span></div>
    <button aria-expanded={expanded} className="grid w-full grid-cols-[minmax(160px,1fr)_auto_auto] items-center gap-4 px-4 py-3 text-left" onClick={() => setExpanded((value) => !value)} type="button"><span><strong className="block text-sm">{t(`trace.boundaries.${kind}`)}</strong><small className="mt-0.5 block text-[10px] text-muted-foreground">{t(`trace.boundaries.${kind}Description`)}</small></span><Badge tone={statusTone(status)}>{t(`common.${status}`, { defaultValue: status })}</Badge><ChevronDown className={cn('size-4 text-muted-foreground transition-transform', expanded && 'rotate-180')} /></button>
    {expanded && <div className="border-t border-border p-4"><h3 className="mb-3 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{t(kind === 'start' ? 'trace.workflowInput' : 'trace.workflowOutput')}</h3><TraceSemanticValue empty={t(kind === 'start' ? 'trace.authoritativeInputEmpty' : 'trace.authoritativeOutputEmpty')} value={value} /></div>}
  </article>
}
