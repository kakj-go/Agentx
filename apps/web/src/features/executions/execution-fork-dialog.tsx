import { AlertTriangle, Braces, GitFork, Play, Recycle, ShieldAlert } from 'lucide-react'
import { useEffect, useMemo, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import type { Checkpoint, ForkExecutionRequest, NodeExecution } from '../../shared/api/types'
import { cn } from '../../shared/lib/cn'
import { Button } from '../../shared/ui/button'
import { Dialog, DialogContent } from '../../shared/ui/dialog'
import { Select } from '../../shared/ui/select'
import { Textarea } from '../../shared/ui/textarea'

type ForkMode = 'whole' | 'node' | 'to_node' | 'from_node'
type Decision = 'execute' | 'reuse_output' | 'dry_run'

type ForkDialogProps = {
  open: boolean
  nodes: NodeExecution[]
  checkpoints: Checkpoint[]
  initialNodeId?: string
  pending: boolean
  onClose: () => void
  onSubmit: (request: ForkExecutionRequest) => Promise<void>
}

const modes: ForkMode[] = ['whole', 'node', 'to_node', 'from_node']

export function ExecutionForkDialog({ open, nodes, checkpoints, initialNodeId, pending, onClose, onSubmit }: ForkDialogProps) {
  const { t } = useTranslation()
  const uniqueNodes = useMemo(() => Array.from(new Map(nodes.map((node) => [node.nodeId, node])).values()), [nodes])
  const [mode, setMode] = useState<ForkMode>('whole')
  const [checkpointId, setCheckpointId] = useState('')
  const [nodeId, setNodeId] = useState('')
  const [overrides, setOverrides] = useState('{}')
  const [error, setError] = useState('')
  const [decisions, setDecisions] = useState<Record<string, Decision>>({})

  useEffect(() => {
    if (!open) return
    setCheckpointId(checkpoints.at(-1)?.id ?? '')
    setNodeId(initialNodeId ?? uniqueNodes[0]?.nodeId ?? '')
    setOverrides('{}')
    setError('')
    setDecisions(Object.fromEntries(uniqueNodes.filter((node) => node.sideEffectLevel === 'irreversible').map((node) => [node.nodeId, 'dry_run'])))
  }, [checkpoints, initialNodeId, open, uniqueNodes])

  const selectedIndex = uniqueNodes.findIndex((node) => node.nodeId === nodeId)
  const rerun = uniqueNodes.filter((_, index) => mode === 'whole' || (mode === 'node' && index === selectedIndex) || (mode === 'to_node' && index <= selectedIndex) || (mode === 'from_node' && index >= selectedIndex))
  const reused = uniqueNodes.filter((node) => !rerun.includes(node))
  const risky = rerun.filter((node) => node.sideEffectLevel === 'irreversible')

  const submit = async () => {
    try {
      const inputOverrides = JSON.parse(overrides) as unknown
      if (!checkpointId) throw new Error(t('executions.forkDialog.selectCheckpoint'))
      if (mode !== 'whole' && !nodeId) throw new Error(t('executions.forkDialog.selectNode'))
      await onSubmit({
        checkpointId,
        mode,
        nodeId: mode === 'whole' ? null : nodeId,
        inputOverrides,
        sideEffectDecisions: Object.fromEntries(risky.map((node) => [node.nodeId, decisions[node.nodeId] ?? 'dry_run'])),
        idempotencyKey: crypto.randomUUID(),
      })
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    }
  }

  return <Dialog onOpenChange={(value) => { if (!value && !pending) onClose() }} open={open}>
    <DialogContent className="w-[min(920px,calc(100vw-32px))]" description={t('executions.forkDialog.description')} title={t('executions.forkDialog.title')}>
      <div className="flex items-center gap-3 border-b border-border px-5 py-4"><span className="grid size-9 place-items-center rounded-md bg-primary/10 text-primary"><GitFork className="size-4.5" /></span><div><h2 className="text-sm font-semibold">{t('executions.forkDialog.title')}</h2><p className="mt-0.5 text-[11px] text-muted-foreground">{t('executions.forkDialog.intro')}</p></div></div>
      <div className="grid grid-cols-[minmax(260px,0.75fr)_minmax(0,1.25fr)] max-md:grid-cols-1">
        <div className="space-y-4 border-r border-border p-5 max-md:border-b max-md:border-r-0">
          <Field label={t('executions.forkDialog.checkpoint')}><Select aria-label={t('executions.forkDialog.checkpoint')} className="w-full" onValueChange={setCheckpointId} options={checkpoints.map((checkpoint) => ({ value: checkpoint.id, label: `#${checkpoint.sequenceNumber} · ${checkpoint.checkpointType}` }))} value={checkpointId} /></Field>
          <Field label={t('executions.forkDialog.scope')}><div className="grid grid-cols-2 gap-1 rounded-md bg-muted p-1">{modes.map((item) => <button className={cn('h-8 rounded text-[11px] font-medium text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-primary/30', mode === item && 'bg-surface text-foreground shadow-sm')} key={item} onClick={() => setMode(item)} type="button">{t(`executions.forkDialog.modes.${item}`)}</button>)}</div></Field>
          {mode !== 'whole' && <Field label={t('executions.forkDialog.targetNode')}><Select aria-label={t('executions.forkDialog.targetNode')} className="w-full" onValueChange={setNodeId} options={uniqueNodes.map((node) => ({ value: node.nodeId, label: `${node.nodeName} · ${t('executions.forkDialog.run')} ${node.runIndex}` }))} value={nodeId} /></Field>}
          <Field label={t('executions.forkDialog.inputOverrides')}><Textarea aria-label={t('executions.forkDialog.inputOverrides')} className="min-h-32 font-mono text-[11px]" onChange={(event) => setOverrides(event.target.value)} spellCheck={false} value={overrides} /></Field>
          {error && <p className="flex gap-2 text-[11px] text-danger"><AlertTriangle className="mt-0.5 size-3.5 shrink-0" />{error}</p>}
        </div>
        <div className="min-w-0 p-5">
          <div className="mb-3 flex items-center justify-between"><h3 className="text-xs font-semibold">{t('executions.forkDialog.preview')}</h3><span className="text-[10px] text-muted-foreground">{t('executions.forkDialog.previewSummary', { rerun: rerun.length, reuse: reused.length })}</span></div>
          <div className="max-h-[440px] overflow-auto border-y border-border">
            {uniqueNodes.map((node) => {
              const willRerun = rerun.includes(node)
              const irreversible = willRerun && node.sideEffectLevel === 'irreversible'
              return <div className="grid grid-cols-[28px_minmax(0,1fr)_150px] items-center gap-3 border-b border-border px-2 py-3 last:border-b-0 max-sm:grid-cols-[28px_minmax(0,1fr)]" key={node.nodeId}>
                <span className={cn('grid size-7 place-items-center rounded-md', willRerun ? 'bg-primary/10 text-primary' : 'bg-success/10 text-success')}>{willRerun ? <Play className="size-3.5" /> : <Recycle className="size-3.5" />}</span>
                <div className="min-w-0"><strong className="block truncate text-[11px]">{node.nodeName}</strong><span className="text-[9px] text-muted-foreground">{willRerun ? t('executions.forkDialog.rerun') : t('executions.forkDialog.reuseOutput')} · {t(`executions.sideEffects.${node.sideEffectLevel}`)}</span></div>
                {irreversible ? <Select aria-label={t('executions.forkDialog.decision', { name: node.nodeName })} className="h-8 min-w-0 text-[11px] max-sm:col-start-2" onValueChange={(value) => setDecisions((current) => ({ ...current, [node.nodeId]: value as Decision }))} options={[{ value: 'dry_run', label: t('executions.forkDialog.dryRun') }, { value: 'reuse_output', label: t('executions.forkDialog.reusePreviousOutput'), disabled: node.output == null }, { value: 'execute', label: t('executions.forkDialog.confirmExecute') }]} value={decisions[node.nodeId] ?? 'dry_run'} /> : <span className="text-right text-[10px] text-muted-foreground max-sm:hidden">{willRerun ? t('executions.forkDialog.willExecute') : t('executions.forkDialog.noSideEffect')}</span>}
              </div>
            })}
          </div>
          {risky.length > 0 && <div className="mt-4 flex gap-2 border-l-2 border-warning bg-warning/10 px-3 py-2.5 text-[10px] leading-4 text-foreground"><ShieldAlert className="mt-0.5 size-4 shrink-0 text-warning" /><span>{t('executions.forkDialog.risk', { count: risky.length })}</span></div>}
          {!uniqueNodes.length && <div className="grid min-h-48 place-items-center text-xs text-muted-foreground"><Braces className="mb-2 size-5" />{t('executions.forkDialog.noNodes')}</div>}
        </div>
      </div>
      <div className="flex justify-end gap-2 border-t border-border px-5 py-4"><Button disabled={pending} onClick={onClose} variant="secondary">{t('common.cancel')}</Button><Button disabled={pending || !checkpointId || !uniqueNodes.length} onClick={() => void submit()}><GitFork className="size-4" />{pending ? t('executions.forkDialog.creating') : t('executions.forkDialog.create')}</Button></div>
    </DialogContent>
  </Dialog>
}

function Field({ label, children }: { label: string; children: ReactNode }) {
  return <label className="block"><span className="mb-1.5 block text-[10px] font-semibold text-muted-foreground">{label}</span>{children}</label>
}
