import { Braces, X } from 'lucide-react'
import { createPortal } from 'react-dom'
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type RefObject } from 'react'
import type { TFunction } from 'i18next'
import { useTranslation } from 'react-i18next'

import { Button } from '../../../../shared/ui/button'
import { useDialogLayer } from '../../../../shared/ui/dialog'
import type { ReferenceCatalog, ReferenceEntry, ReferenceNamespace } from './reference-types'
import { ReferenceTree } from './reference-tree'

const ROOTS: ReferenceNamespace[] = ['inputs', 'outputs', 'contexts', 'item', 'execution', 'loop']
const PICKER_GAP = 6
const PICKER_HEIGHT = 300
const PICKER_MAX_WIDTH = 440
const VIEWPORT_PADDING = 16

export function ReferencePicker({ catalog, allowedNamespaces = ['inputs', 'outputs', 'contexts'], expectedType, open, onOpenChange, onInsert, anchorRef }: { catalog: ReferenceCatalog; allowedNamespaces?: ReferenceNamespace[]; expectedType?: string; open: boolean; onOpenChange: (open: boolean) => void; onInsert: (expression: string) => void; anchorRef?: RefObject<HTMLElement | null> }) {
  const { t } = useTranslation()
  const dialogLayer = useDialogLayer()
  const root = useRef<HTMLDivElement>(null)
  const [position, setPosition] = useState({ height: PICKER_HEIGHT, left: VIEWPORT_PADDING, top: 80, width: PICKER_MAX_WIDTH })
  const [namespace, setNamespace] = useState<ReferenceNamespace>()
  const entries = useMemo(() => markCompatibility(namespace ? catalog[namespace] ?? [] : [], expectedType, t), [catalog, expectedType, namespace, t])
  useEffect(() => {
    if (!open) return
    const close = (event: MouseEvent) => { const target = event.target as Node; if (!root.current?.contains(target) && !anchorRef?.current?.contains(target)) onOpenChange(false) }
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') onOpenChange(false) }
    document.addEventListener('mousedown', close)
    document.addEventListener('keydown', escape)
    return () => { document.removeEventListener('mousedown', close); document.removeEventListener('keydown', escape) }
  }, [anchorRef, onOpenChange, open])
  useEffect(() => { if (!open) setNamespace(undefined) }, [open])
  useLayoutEffect(() => {
    if (!open || !anchorRef?.current) return
    const updatePosition = () => {
      const rect = anchorRef.current?.getBoundingClientRect()
      if (!rect) return
      const layerRect = dialogLayer?.getBoundingClientRect()
      const viewportWidth = window.innerWidth - VIEWPORT_PADDING * 2
      const width = Math.min(PICKER_MAX_WIDTH, Math.max(320, rect.width), viewportWidth)
      const preferredLeft = rect.left + width <= window.innerWidth - VIEWPORT_PADDING
        ? rect.left
        : rect.right - width
      const left = Math.min(
        Math.max(VIEWPORT_PADDING, preferredLeft),
        window.innerWidth - VIEWPORT_PADDING - width,
      )
      const spaceBelow = window.innerHeight - VIEWPORT_PADDING - rect.bottom - PICKER_GAP
      const spaceAbove = rect.top - VIEWPORT_PADDING - PICKER_GAP
      const below = spaceBelow >= PICKER_HEIGHT || spaceBelow >= spaceAbove
      const height = Math.min(PICKER_HEIGHT, Math.max(160, below ? spaceBelow : spaceAbove))
      const top = below ? rect.bottom + PICKER_GAP : rect.top - PICKER_GAP - height
      setPosition({
        height,
        left: left - (layerRect?.left ?? 0),
        top: top - (layerRect?.top ?? 0),
        width,
      })
    }
    updatePosition()
    window.addEventListener('resize', updatePosition)
    window.addEventListener('scroll', updatePosition, true)
    return () => {
      window.removeEventListener('resize', updatePosition)
      window.removeEventListener('scroll', updatePosition, true)
    }
  }, [anchorRef, dialogLayer, open])
  if (!open) return null
  const choose = (entry: ReferenceEntry) => { if (entry.expression && !entry.sensitive) { onInsert(entry.expression); onOpenChange(false) } }
  const picker = <div className={`${dialogLayer ? 'absolute' : 'fixed'} pointer-events-auto z-[100] grid grid-cols-[148px_minmax(0,1fr)] overflow-hidden rounded-md border border-border bg-surface shadow-xl`} data-testid="reference-picker" ref={root} style={position}>
    <div className="min-h-0 border-r border-border"><div className="flex h-10 items-center border-b border-border px-2 text-xs font-semibold"><Braces className="mr-2 size-3.5 text-primary" />{t('studio.references.title')}</div><ReferenceTree entries={ROOTS.filter((item) => allowedNamespaces.includes(item) && (catalog[item]?.length ?? 0) > 0).map((item) => ({ id: item, label: t(`studio.references.${item}`, item), path: item, children: catalog[item] ?? [] }))} selected={namespace} onSelect={(entry) => setNamespace(entry.id as ReferenceNamespace)} /></div>
    <div className="min-h-0 min-w-0"><div className="flex h-10 items-center gap-1 border-b border-border px-1"><span className="min-w-0 flex-1 truncate px-1 text-[11px] text-muted-foreground">{namespace ?? t('studio.references.selectNamespace')}</span><Button aria-label={t('common.close')} onClick={() => onOpenChange(false)} size="icon" variant="ghost"><X className="size-3.5" /></Button></div><ReferenceTree entries={entries} expandable key={namespace} onSelect={choose} /></div>
  </div>
  return createPortal(picker, dialogLayer ?? document.body)
}

function markCompatibility(entries: ReferenceEntry[], expectedType: string | undefined, t: TFunction): ReferenceEntry[] {
  return entries.map((entry) => {
    const children = markCompatibility(entry.children, expectedType, t)
    const incompatible = Boolean(expectedType && entry.expression && entry.type && !compatible(entry.type, expectedType))
    return { ...entry, children, disabledReason: entry.sensitive ? t('studio.references.sensitiveDisabled') : incompatible ? t('studio.references.typeMismatch', { expected: expectedType, actual: entry.type }) : entry.disabledReason }
  })
}

function compatible(actual: string, expected: string) {
  if (actual === expected) return true
  if (expected === 'number' && actual === 'integer') return true
  return false
}
