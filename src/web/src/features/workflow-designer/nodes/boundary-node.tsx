import { Handle, Position, type NodeProps } from '@xyflow/react'
import { Flag, Home } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { cn } from '../../../shared/lib/cn'
import type { CanvasNode } from '../model/types'
import { STUDIO_CARD_WIDTH } from './node-appearance'

export function BoundaryNode({ data, selected }: NodeProps<CanvasNode>) {
  const { t } = useTranslation()
  if (data.editorKind !== 'boundary') return null
  const start = data.boundary === 'start'
  return <div className="relative shrink-0" data-testid={`workflow-${data.boundary}`} style={{ width: STUDIO_CARD_WIDTH, height: 62 }}>
    <div className={cn('studio-card flex size-full flex-col overflow-hidden rounded-xl border border-border bg-surface text-foreground shadow-sm transition-shadow', selected && 'studio-node-selected')}>
      <div className="flex h-11 shrink-0 items-center gap-2 px-3">
        <span className={cn('grid size-6 shrink-0 place-items-center rounded-md text-white', start ? 'bg-[#155EEF]' : 'bg-[#F59E0B]')}>
          {start ? <Home className="size-3.5" /> : <Flag className="size-3.5" />}
        </span>
        <strong className="min-w-0 flex-1 truncate text-[13px] font-semibold leading-none">{start ? t('studio.boundary.start') : t('studio.boundary.end')}</strong>
      </div>
      <div className="flex h-[18px] items-center px-3 text-[11px] leading-none text-muted-foreground">
        <span className="min-w-0 flex-1 truncate">{start ? t('studio.boundary.startSummary') : t('studio.boundary.endSummary')}</span>
      </div>
    </div>
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
    <Handle aria-label={label} className={cn('studio-handle !absolute !grid !size-4 !place-items-center !border-0 !bg-transparent', `studio-boundary-port-${tone}`)} data-testid={`boundary-port-${id}`} id={id} position={position} style={{ top }} type={type}><span className="studio-handle-mark block rounded-[2px]" /></Handle>
    <span className={cn('pointer-events-none absolute flex h-4 -translate-y-1/2 items-center whitespace-nowrap text-[9px] font-medium leading-none text-muted-foreground', labelPosition)} data-testid={`boundary-port-label-${id}`} style={{ top }}>{label}</span>
  </>
}
