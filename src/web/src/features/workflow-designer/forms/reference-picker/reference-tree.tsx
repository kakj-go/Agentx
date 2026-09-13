import { ChevronDown, ChevronRight, CircleAlert } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { sourceNodeColor } from './reference-color'
import type { ReferenceEntry } from './reference-types'

export function ReferenceTree({ entries, selected, onSelect, expandable = false, grouped = false }: { entries: ReferenceEntry[]; selected?: string; onSelect: (entry: ReferenceEntry) => void; expandable?: boolean; grouped?: boolean }) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set())
  const activate = (entry: ReferenceEntry, toggleOnly = false) => {
    if (!expandable || entry.children.length === 0 || (entry.selector && !toggleOnly)) {
      onSelect(entry)
      return
    }
    setExpanded((current) => {
      const next = new Set(current)
      if (next.has(entry.id)) next.delete(entry.id)
      else next.add(entry.id)
      return next
    })
  }
  return <div className="h-[calc(100%-40px)] min-h-0 overflow-y-auto p-1">{entries.map((entry) => grouped
    ? <SourceNodeGroup entry={entry} expanded={expanded} key={entry.id} onActivate={activate} recommendedLabel={t('studio.references.recommended')} selected={selected} />
    : <TreeRow entry={entry} expanded={expanded} key={entry.id} onActivate={activate} recommendedLabel={t('studio.references.recommended')} selected={selected} />)}</div>
}

function SourceNodeGroup({ entry, expanded, selected, onActivate, recommendedLabel }: { entry: ReferenceEntry; expanded: Set<string>; selected?: string; onActivate: (entry: ReferenceEntry, toggleOnly?: boolean) => void; recommendedLabel: string }) {
  return <>
    <div className="flex h-8 items-center gap-2 rounded px-2 text-[11px] font-semibold" data-testid={`reference-group-${entry.label}`}>
      <span className="size-2 shrink-0 rounded-full" style={{ backgroundColor: sourceNodeColor(entry.sourceNodeType) }} />
      <span className="min-w-0 flex-1 truncate">{entry.label}</span>
      <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] font-normal text-muted-foreground">{entry.children.length}</span>
    </div>
    {entry.children.map((child) => <TreeRow depth={1} entry={child} expanded={expanded} key={child.id} onActivate={onActivate} recommendedLabel={recommendedLabel} selected={selected} />)}
  </>
}

function TreeRow({ entry, expanded, selected, onActivate, recommendedLabel, depth = 0 }: { entry: ReferenceEntry; expanded: Set<string>; selected?: string; onActivate: (entry: ReferenceEntry, toggleOnly?: boolean) => void; recommendedLabel: string; depth?: number }) {
  const branch = entry.children.length > 0
  const open = branch && expanded.has(entry.id)
  const meta = entry.type ?? entry.cardinality
  return <>
    <button
      aria-expanded={branch ? open : undefined}
      aria-disabled={entry.disabledReason && !branch ? true : undefined}
      className={`flex h-8 w-full items-center gap-2 rounded pr-2 text-left text-xs ${entry.disabledReason ? 'cursor-not-allowed text-muted-foreground/60' : selected === entry.id ? 'bg-primary/10 text-primary' : 'text-foreground hover:bg-muted'}`}
      disabled={Boolean(entry.disabledReason && !branch)}
      onClick={(event) => {
        const toggle = Boolean((event.target as Element).closest('[data-tree-toggle]')) || Boolean(entry.disabledReason && branch)
        onActivate(entry, toggle)
      }}
      style={{ paddingLeft: 8 + depth * 16 }}
      title={entry.disabledReason}
      type="button"
    >
      {branch && <span data-tree-toggle>{open ? <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" /> : <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" />}</span>}
      {!branch && <span className="size-3.5 shrink-0" />}
      <span className="min-w-0 flex-1 truncate">{entry.label}</span>
      {entry.conversionNote ? <span className="max-w-24 truncate rounded bg-primary/10 px-1.5 py-0.5 text-[10px] text-primary">{entry.conversionNote}</span> : entry.recommended ? <span className="max-w-20 truncate rounded bg-success/10 px-1.5 py-0.5 text-[10px] text-success">{recommendedLabel}</span> : meta && <span className="max-w-20 truncate rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">{meta}</span>}
      {entry.disabledReason && <CircleAlert className="size-3.5 shrink-0 text-warning" />}
    </button>
    {open && entry.children.map((child) => <TreeRow depth={depth + 1} entry={child} expanded={expanded} key={child.id} onActivate={onActivate} recommendedLabel={recommendedLabel} selected={selected} />)}
  </>
}
