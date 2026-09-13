import { AlertCircle, CheckCircle2 } from 'lucide-react'
import { type ReactNode, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { Button, type ButtonProps } from '../ui/button'
import { Dialog, DialogContent } from '../ui/dialog'

export type Prerequisite = {
  key: string
  label: string
  met: boolean
  actionLabel?: string
  href?: string
  onAction?: () => void
}

type PrerequisiteActionProps = Omit<ButtonProps, 'onClick'> & {
  children: ReactNode
  description?: string
  loading?: boolean
  onReady: () => void
  requirements: Prerequisite[]
}

export function PrerequisiteAction({ children, description, loading, onReady, requirements, ...buttonProps }: PrerequisiteActionProps) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const missing = requirements.filter((requirement) => !requirement.met)
  const activate = () => missing.length === 0 ? onReady() : setOpen(true)
  return <>
    <Button {...buttonProps} disabled={loading || buttonProps.disabled} onClick={activate}>{children}</Button>
    <Dialog onOpenChange={setOpen} open={open}>
      <DialogContent description={description ?? t('common.prerequisites.description')} title={t('common.prerequisites.title')}>
        <div className="p-5">
          <div className="flex items-start gap-3">
            <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-warning/10 text-warning"><AlertCircle className="size-4" /></span>
            <div><h2 className="text-sm font-semibold">{t('common.prerequisites.title')}</h2><p className="mt-1 text-xs leading-5 text-muted-foreground">{description ?? t('common.prerequisites.description')}</p></div>
          </div>
          <div className="mt-5 divide-y divide-border rounded-lg border border-border">
            {requirements.map((requirement) => <div className="flex min-h-14 items-center gap-3 px-4" key={requirement.key}>
              <CheckCircle2 className={`size-4 shrink-0 ${requirement.met ? 'text-success' : 'text-muted-foreground'}`} />
              <span className="min-w-0 flex-1 text-xs">{requirement.label}</span>
              {!requirement.met && requirement.href && requirement.actionLabel && <Button asChild size="sm" variant="secondary"><Link onClick={() => setOpen(false)} to={requirement.href}>{requirement.actionLabel}</Link></Button>}
              {!requirement.met && requirement.onAction && requirement.actionLabel && <Button onClick={() => { setOpen(false); requirement.onAction?.() }} size="sm" variant="secondary">{requirement.actionLabel}</Button>}
              {!requirement.met && !requirement.href && !requirement.onAction && <span className="text-[11px] text-muted-foreground">{t('common.prerequisites.contactAdmin')}</span>}
            </div>)}
          </div>
          <div className="mt-5 flex justify-end"><Button onClick={() => setOpen(false)} variant="ghost">{t('common.close')}</Button></div>
        </div>
      </DialogContent>
    </Dialog>
  </>
}
