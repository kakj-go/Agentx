import { Search } from 'lucide-react'
import { useMemo, useState, type DragEvent } from 'react'
import { useTranslation } from 'react-i18next'

import { Input } from '../../../shared/ui/input'
import type { NodeManifest, ResourceType } from '../model/types'
import { NodeIcon } from '../nodes/node-icon'
import { categoryLabel, nodeCategory, resourceIcon, type NodeCategory } from '../nodes/node-appearance'

export function NodePalette({ manifests, onAddAction, onAddBinding }: { manifests: NodeManifest[]; onAddAction: (manifest: NodeManifest) => void; onAddBinding: (resourceType: ResourceType, role: string) => void }) {
  const { t } = useTranslation()
  const [search, setSearch] = useState('')
  const visible = useMemo(() => manifests.filter((manifest) => `${manifest.displayName} ${manifest.nodeType} ${manifest.keywords.join(' ')}`.toLowerCase().includes(search.toLowerCase())), [manifests, search])
  const grouped = useMemo(() => {
    const groups = new Map<NodeCategory, NodeManifest[]>()
    for (const manifest of visible) {
      const category = nodeCategory(manifest)
      groups.set(category, [...(groups.get(category) ?? []), manifest])
    }
    return groups
  }, [visible])
  const attachments = useMemo(() => {
    const values = new Map<ResourceType, string>()
    for (const manifest of manifests) for (const slot of manifest.bindingSlots) values.set(slot.resourceType, slot.name)
    return [...values]
  }, [manifests])
  const drag = (event: DragEvent, payload: object) => { event.dataTransfer.setData('application/agentx-studio', JSON.stringify(payload)); event.dataTransfer.effectAllowed = 'copy' }
  return <aside className="flex w-64 shrink-0 flex-col border-r border-border bg-surface">
    <div className="border-b border-border bg-surface p-3"><label className="relative block"><Search className="pointer-events-none absolute left-2.5 top-2.5 size-3.5 text-muted-foreground" /><Input aria-label={t('studio.palette.search')} className="h-9 border-border/80 bg-canvas pl-8 text-xs" onChange={(event) => setSearch(event.target.value)} placeholder={t('studio.palette.search')} value={search} /></label><p className="mt-2 text-[10px] text-muted-foreground">{t('studio.palette.dragHint')}</p></div>
    <div className="min-h-0 flex-1 overflow-y-auto p-3">
      {[...grouped.entries()].map(([category, items]) => <section className="mb-5" key={category}><PaletteHeading count={items.length}>{categoryLabel(category, t)}</PaletteHeading><div className="mt-2 space-y-1">{items.map((manifest) => <PaletteItem data-testid={`palette-action-${manifest.nodeType}`} draggable icon={manifest.iconKey} key={`${manifest.nodeType}@${manifest.version}`} label={manifest.displayName} meta={manifest.description || manifest.capability} onClick={() => onAddAction(manifest)} onDragStart={(event) => drag(event, { kind: 'action', nodeType: manifest.nodeType, version: manifest.version })} />)}</div></section>)}
      <section><PaletteHeading count={attachments.length}>{t('studio.palette.attachments')}</PaletteHeading><div className="mt-2 space-y-1">{attachments.map(([resourceType, role]) => <PaletteItem data-testid={`palette-binding-${resourceType}`} draggable icon={resourceIcon(resourceType)} key={resourceType} label={t(`resourceTypes.${resourceType}`)} meta={role.replaceAll('_', ' ')} onClick={() => onAddBinding(resourceType, role)} onDragStart={(event) => drag(event, { kind: 'binding', resourceType, role })} />)}</div></section>
    </div>
  </aside>
}

function PaletteHeading({ children, count }: { children: string; count: number }) { return <h2 className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-[0.08em] text-muted-foreground"><span className="size-1.5 rounded-full bg-primary/70" />{children}<span className="ml-auto rounded-full bg-muted px-1.5 py-0.5 text-[9px] font-medium normal-case tracking-normal">{count}</span></h2> }
function PaletteItem({ icon, label, meta, ...props }: { icon: string; label: string; meta: string } & React.ButtonHTMLAttributes<HTMLButtonElement>) { return <button className="group flex w-full items-center gap-2 rounded-md border border-transparent px-2 py-2 text-left transition-colors hover:border-border hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30" type="button" {...props}><span className="grid size-8 shrink-0 place-items-center rounded-lg bg-primary/10 text-primary transition-transform group-hover:scale-105"><NodeIcon className="size-3.5" iconKey={icon} /></span><span className="min-w-0"><strong className="block truncate text-[11px] font-medium">{label}</strong><span className="block truncate text-[9px] text-muted-foreground">{meta}</span></span></button> }
