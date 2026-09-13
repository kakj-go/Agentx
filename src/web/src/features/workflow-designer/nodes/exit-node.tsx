import { Handle, Position, type NodeProps } from '@xyflow/react'
import { Flag, Lock } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { cn } from '../../../shared/lib/cn'
import { localizedExitLabel } from '../model/node-display'
import type { CanvasNode, ExitNodeData } from '../model/types'
import { STUDIO_CARD_WIDTH } from './node-appearance'

export function ExitNode({ data, selected }: NodeProps<CanvasNode>) {
  const { t } = useTranslation()
  if (data.editorKind !== 'exit') return null
  const exit = data as ExitNodeData
  const outputs = Object.keys(exit.parameters.outputs).length
  return <div className="relative shrink-0" data-testid={`exit-node-${exit.key}`} style={{ width: STUDIO_CARD_WIDTH, height: 62 }}>
    <div className={cn('studio-card flex size-full flex-col overflow-hidden rounded-xl border border-border bg-surface text-foreground shadow-sm transition-shadow', selected && 'studio-node-selected')}>
      <div className="flex h-11 shrink-0 items-center gap-2 px-3">
        <span className="grid size-6 shrink-0 place-items-center rounded-md bg-[#F59E0B] text-white"><Flag className="size-3.5" /></span>
        <strong className="flex min-w-0 flex-1 items-center gap-1 text-[13px] font-semibold leading-none">
          <span className="min-w-0 truncate">{localizedExitLabel(exit.label, t('studio.exit.defaultName'))}</span>
          {exit.protected ? <Lock aria-label={t('studio.exit.protected')} className="size-3 shrink-0 text-muted-foreground" data-testid="exit-protected" /> : null}
        </strong>
      </div>
      <div className="flex h-[18px] items-center px-3 text-[11px] leading-none text-muted-foreground">
        <span className="min-w-0 flex-1 truncate">{t('studio.exit.summary')}{outputs > 0 ? ` ×${outputs}` : ''}</span>
      </div>
    </div>
    <ExitPort id="main" label={t('studio.boundary.output')} tone="primary" top="30%" />
    <ExitPort id="error" label={t('studio.boundary.error')} tone="danger" top="70%" />
  </div>
}

function ExitPort({ id, label, tone, top }: { id: string; label: string; tone: 'primary' | 'danger'; top: string }) {
  return <>
    <Handle aria-label={label} className={cn('studio-handle !absolute !grid !size-4 !place-items-center !border-0 !bg-transparent', `studio-boundary-port-${tone}`)} data-testid={`exit-port-${id}`} id={id} position={Position.Left} style={{ top }} type="target"><span className="studio-handle-mark block rounded-[2px]" /></Handle>
    <span className={cn('pointer-events-none absolute right-full mr-3 flex h-4 -translate-y-1/2 items-center whitespace-nowrap text-[9px] font-medium leading-none text-muted-foreground')} data-testid={`exit-port-label-${id}`} style={{ top }}>{label}</span>
  </>
}
