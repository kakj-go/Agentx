import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '../ui/button'
import { Tooltip } from '../ui/tooltip'
import { EntityDeleteDialog } from './entity-delete-dialog'

type EntityDeleteButtonProps = {
  canDelete: boolean
  deletePath: string
  entityId: string
  entityName: string
  entityType: string
  immutableReason?: string
  onDeleted: () => Promise<void> | void
}

export function EntityDeleteButton({ canDelete, deletePath, entityId, entityName, entityType, immutableReason, onDeleted }: EntityDeleteButtonProps) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  if (!canDelete) return null
  const button = <Button aria-label={immutableReason} className="text-danger hover:bg-danger/10 hover:text-danger" disabled={Boolean(immutableReason)} onClick={() => setOpen(true)} size="sm" title={immutableReason} variant="ghost">{t('common.delete')}</Button>
  return <>
    {immutableReason ? <Tooltip content={immutableReason}><span className="inline-flex">{button}</span></Tooltip> : button}
    {open && <EntityDeleteDialog deletePath={deletePath} entityId={entityId} entityName={entityName} entityType={entityType} onClose={() => setOpen(false)} onDeleted={onDeleted} open />}
  </>
}
