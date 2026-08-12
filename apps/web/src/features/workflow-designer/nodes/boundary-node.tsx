import { Handle, Position, type NodeProps } from '@xyflow/react'
import { LogIn, LogOut } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { cn } from '../../../shared/lib/cn'
import type { CanvasNode } from '../model/types'

export function BoundaryNode({ data, selected }: NodeProps<CanvasNode>) {
  const { t } = useTranslation()
  if (data.editorKind !== 'boundary') return null
  const start = data.boundary === 'start'
  const Icon = start ? LogIn : LogOut
  return <div className={cn('relative flex h-20 w-40 items-center gap-3 rounded-md border bg-surface px-3 shadow-sm', selected ? 'border-primary ring-2 ring-primary/15' : 'border-border')} data-testid={`workflow-${data.boundary}`}>
    <span className={cn('grid size-10 shrink-0 place-items-center rounded-md', start ? 'bg-success/10 text-success' : 'bg-primary/10 text-primary')}><Icon className="size-5" /></span>
    <div className="min-w-0"><strong className="block text-xs">{start ? t('studio.boundary.start') : t('studio.boundary.end')}</strong><span className="block truncate text-[9px] text-muted-foreground">{start ? t('studio.boundary.startSummary') : t('studio.boundary.endSummary')}</span></div>
    {start
      ? <BoundaryPort id="main" label={t('studio.boundary.input')} position={Position.Right} tone="success" top="50%" type="source" />
      : <>
          <BoundaryPort id="main" label={t('studio.boundary.output')} position={Position.Left} tone="primary" top="30%" type="target" />
          <BoundaryPort id="error" label={t('studio.boundary.error')} position={Position.Left} tone="danger" top="70%" type="target" />
        </>}
  </div>
}

function BoundaryPort({ id, label, position, tone, top, type }: { id: string; label: string; position: Position; tone: 'success' | 'primary' | 'danger'; top: string; type: 'source' | 'target' }) {
  const labelPosition = position === Position.Left ? 'right-full mr-3' : 'left-full ml-3'
  return <>
    <Handle aria-label={label} className={cn('!absolute !grid !size-4 !place-items-center !border-0 !bg-transparent', `studio-boundary-port-${tone}`)} data-testid={`boundary-port-${id}`} id={id} position={position} style={{ top }} type={type}><span className={cn('block size-3 rounded-full border-2 border-background', tone === 'success' ? 'bg-success' : tone === 'danger' ? 'bg-danger' : 'bg-primary')} /></Handle>
    <span className={cn('pointer-events-none absolute flex h-4 -translate-y-1/2 items-center whitespace-nowrap text-[9px] font-medium leading-none text-muted-foreground', labelPosition)} data-testid={`boundary-port-label-${id}`} style={{ top }}>{label}</span>
  </>
}
