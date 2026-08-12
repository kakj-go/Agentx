import { Box, CircleDot, RotateCcw } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { NodeExecution } from '../../shared/api/types'
import { cn } from '../../shared/lib/cn'
import { localizedValue } from '../../shared/lib/localized-value'
import { executionStatus } from './execution-format'

type OutlineProps = {
  nodes: NodeExecution[]
  selectedId?: string
  onSelect: (id: string) => void
}

const statusTone = {
  success: 'bg-success', running: 'bg-primary', waiting: 'bg-warning', failed: 'bg-danger', pending: 'bg-muted-foreground', inactive: 'bg-muted-foreground',
} as const

export function ExecutionOutline({ nodes, selectedId, onSelect }: OutlineProps) {
  const { t } = useTranslation()
  return <aside aria-label={t('executions.nodePanel.outlineAriaLabel')} className="min-w-0 border-r border-border bg-surface max-lg:border-b max-lg:border-r-0">
    <div className="flex h-12 items-center justify-between border-b border-border px-4"><strong className="text-xs">{t('executions.nodePanel.outline')}</strong><span className="text-[10px] text-muted-foreground">{t('executions.nodePanel.runCount', { count: nodes.length })}</span></div>
    <div className="max-h-[calc(100vh-250px)] overflow-auto p-2 max-lg:flex max-lg:max-h-none max-lg:gap-2">
      {nodes.length ? nodes.map((node) => {
        const status = executionStatus(node.status)
        return <button aria-current={selectedId === node.id ? 'true' : undefined} className={cn('mb-1 flex w-full min-w-0 items-center gap-3 rounded-md px-2.5 py-2.5 text-left outline-none transition-colors hover:bg-muted focus-visible:ring-2 focus-visible:ring-primary/25 max-lg:w-56 max-lg:shrink-0', selectedId === node.id && 'bg-primary/10')} key={node.id} onClick={() => onSelect(node.id)}>
          <span className={cn('relative grid size-8 shrink-0 place-items-center rounded-md bg-muted text-muted-foreground', selectedId === node.id && 'bg-primary/15 text-primary')}><Box className="size-4" /><span className={cn('absolute -right-0.5 -top-0.5 size-2 rounded-full ring-2 ring-surface', statusTone[status])} /></span>
          <span className="min-w-0 flex-1"><strong className="block truncate text-xs">{node.nodeName}</strong><span className="mt-1 flex items-center gap-1.5 text-[10px] text-muted-foreground"><RotateCcw className="size-3" />{t('executions.nodePanel.run')} {node.runIndex}<span>·</span>{localizedValue(t, 'common', node.status)}</span></span>
        </button>
      }) : <div className="grid min-h-48 place-items-center px-4 text-center text-xs text-muted-foreground"><div><CircleDot className="mx-auto mb-3 size-5" />{t('executions.nodePanel.noNodes')}</div></div>}
    </div>
  </aside>
}
