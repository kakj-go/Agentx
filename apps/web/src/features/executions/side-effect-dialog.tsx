import { ShieldAlert } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

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
  const { t } = useTranslation()
  const [decision, setDecision] = useState('dry_run')
  useEffect(() => { if (node) setDecision('dry_run') }, [node])
  return <Dialog onOpenChange={(open) => { if (!open && !pending) onClose() }} open={Boolean(node)}>
    <DialogContent description={t('executions.sideEffectDialog.description')} title={t('executions.sideEffectDialog.title')}>
      <div className="flex items-center gap-3 border-b border-border px-5 py-4"><span className="grid size-9 place-items-center rounded-md bg-warning/15 text-warning"><ShieldAlert className="size-4.5" /></span><div><h2 className="text-sm font-semibold">{t('executions.sideEffectDialog.title')}</h2><p className="mt-0.5 text-[11px] text-muted-foreground">{node?.nodeName}</p></div></div>
      <div className="space-y-4 p-5"><p className="text-xs leading-5 text-muted-foreground">{t('executions.sideEffectDialog.intro')}</p><Select aria-label={t('executions.sideEffectDialog.decision')} className="w-full" onValueChange={setDecision} options={[{ value: 'dry_run', label: t('executions.forkDialog.dryRun') }, { value: 'reuse_output', label: t('executions.forkDialog.reusePreviousOutput') }, { value: 'execute', label: t('executions.forkDialog.confirmExecute') }]} value={decision} /></div>
      <div className="flex justify-end gap-2 border-t border-border px-5 py-4"><Button disabled={pending} onClick={onClose} variant="secondary">{t('common.cancel')}</Button><Button disabled={pending || !node} onClick={() => node && void onSubmit({ nodeExecutionId: node.id, checkpointId: checkpointId ?? null, decision, idempotencyKey: crypto.randomUUID() })}>{pending ? t('executions.sideEffectDialog.submitting') : t('executions.sideEffectDialog.confirm')}</Button></div>
    </DialogContent>
  </Dialog>
}
