import { ShieldAlert } from 'lucide-react'
import { useEffect, useState } from 'react'

import type { NodeExecution, SideEffectConfirmationRequest } from '../../shared/api/types'
import { Button } from '../../shared/ui/button'
import { Dialog, DialogContent } from '../../shared/ui/dialog'
import { Select } from '../../shared/ui/select'

type SideEffectDialogProps = {
  node?: NodeExecution
  checkpointId?: string
  pending: boolean
  onClose: () => void
  onSubmit: (request: SideEffectConfirmationRequest) => Promise<void>
}

export function SideEffectDialog({ node, checkpointId, pending, onClose, onSubmit }: SideEffectDialogProps) {
  const [decision, setDecision] = useState('dry_run')
  useEffect(() => { if (node) setDecision('dry_run') }, [node])
  return <Dialog onOpenChange={(open) => { if (!open && !pending) onClose() }} open={Boolean(node)}>
    <DialogContent description="选择不可逆节点的恢复策略" title="Side effect confirmation">
      <div className="flex items-center gap-3 border-b border-border px-5 py-4"><span className="grid size-9 place-items-center rounded-md bg-warning/15 text-warning"><ShieldAlert className="size-4.5" /></span><div><h2 className="text-sm font-semibold">Side effect confirmation</h2><p className="mt-0.5 text-[11px] text-muted-foreground">{node?.nodeName}</p></div></div>
      <div className="space-y-4 p-5"><p className="text-xs leading-5 text-muted-foreground">此 Fork 即将经过不可逆节点。选择执行会再次产生真实副作用；Reuse Output 只复用父 Execution 的结果；Dry Run 仅传递带决策标记的输入。</p><Select aria-label="Side effect decision" className="w-full" onValueChange={setDecision} options={[{ value: 'dry_run', label: 'Dry run' }, { value: 'reuse_output', label: 'Reuse previous output' }, { value: 'execute', label: 'Confirm execute' }]} value={decision} /></div>
      <div className="flex justify-end gap-2 border-t border-border px-5 py-4"><Button disabled={pending} onClick={onClose} variant="secondary">取消</Button><Button disabled={pending || !node} onClick={() => node && void onSubmit({ nodeExecutionId: node.id, checkpointId: checkpointId ?? null, decision, idempotencyKey: crypto.randomUUID() })}>{pending ? '提交中…' : '确认决策'}</Button></div>
    </DialogContent>
  </Dialog>
}
