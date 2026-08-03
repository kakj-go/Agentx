import { Button } from '../ui/button'
import { Dialog, DialogContent } from '../ui/dialog'

type ConfirmDialogProps = {
  cancelLabel: string
  confirmLabel: string
  description: string
  onClose: () => void
  onConfirm: () => Promise<void> | void
  open: boolean
  pending?: boolean
  title: string
  variant?: 'primary' | 'danger'
}

export function ConfirmDialog({ cancelLabel, confirmLabel, description, onClose, onConfirm, open, pending = false, title, variant = 'danger' }: ConfirmDialogProps) {
  return <Dialog onOpenChange={(value) => { if (!value && !pending) onClose() }} open={open}>
    <DialogContent className="p-6" description={description} title={title}>
      <h2 className="text-lg font-semibold">{title}</h2>
      <p className="mt-3 text-sm leading-6 text-muted-foreground">{description}</p>
      <div className="mt-6 flex justify-end gap-2">
        <Button disabled={pending} onClick={onClose} type="button" variant="ghost">{cancelLabel}</Button>
        <Button disabled={pending} onClick={() => void onConfirm()} type="button" variant={variant === 'danger' ? 'danger' : 'primary'}>{confirmLabel}</Button>
      </div>
    </DialogContent>
  </Dialog>
}
