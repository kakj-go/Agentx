import { Handle, Position, type NodeProps } from '@xyflow/react'
import { LogOut, Lock } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { cn } from '../../../shared/lib/cn'
import type { CanvasNode, ExitNodeData } from '../model/types'

export function ExitNode({ data, selected }: NodeProps<CanvasNode>) {
  const { t } = useTranslation()
  if (data.editorKind !== 'exit') return null
  const exit = data as ExitNodeData
  return <div className={cn('relative flex h-20 w-40 items-center gap-3 rounded-md border bg-surface px-3 shadow-sm', selected ? 'border-primary ring-2 ring-primary/15' : 'border-border')} data-testid={`exit-node-${exit.key}`}>
    <span className="grid size-10 shrink-0 place-items-center rounded-md bg-primary/10 text-primary"><LogOut className="size-5" /></span>
    <div className="min-w-0">
      <strong className="flex items-center gap-1 text-xs">{exit.label}{exit.protected ? <Lock aria-label={t('studio.exit.protected')} className="size-3 text-muted-foreground" data-testid="exit-protected" /> : null}</strong>
      <span className="block truncate text-[9px] text-muted-foreground">{t('studio.exit.summary')}</span>
    </div>
    <ExitPort id="main" label={t('studio.boundary.output')} tone="primary" top="30%" />
    <ExitPort id="error" label={t('studio.boundary.error')} tone="danger" top="70%" />
  </div>
}

function ExitPort({ id, label, tone, top }: { id: string; label: string; tone: 'primary' | 'danger'; top: string }) {
  return <>
    <Handle aria-label={label} className={cn('!absolute !grid !size-4 !place-items-center !border-0 !bg-transparent', `studio-boundary-port-${tone}`)} data-testid={`exit-port-${id}`} id={id} position={Position.Left} style={{ top }} type="target"><span className={cn('block size-3 rounded-full border-2 border-background', tone === 'danger' ? 'bg-danger' : 'bg-primary')} /></Handle>
    <span className={cn('pointer-events-none absolute right-full mr-3 flex h-4 -translate-y-1/2 items-center whitespace-nowrap text-[9px] font-medium leading-none text-muted-foreground')} data-testid={`exit-port-label-${id}`} style={{ top }}>{label}</span>
  </>
}
