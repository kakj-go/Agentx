import { FileText, Layers3, Search, X } from 'lucide-react'
import { useEffect, useMemo, useState, type DragEvent, type KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'

import { Input } from '../../../shared/ui/input'
import { Tooltip } from '../../../shared/ui/tooltip'
import { localizeManifest, manifestSearchText } from '../model/manifest-localization'
import type { CanvasNodeRole, NodeManifest, ResourceType } from '../model/types'
import { categoryLabel, canvasNodeRole, nodeCategory, resourceIcon, type NodeCategory } from '../nodes/node-appearance'
import { NodeIcon } from '../nodes/node-icon'

type PaletteProps = { manifests: NodeManifest[]; sourceConnection?: { manifest: NodeManifest; handleId: string }; open?: boolean; onOpenChange?: (open: boolean) => void; onAddAction: (manifest: NodeManifest) => void; onAddBinding: (resourceType: ResourceType, role: string) => void; onAddAnnotation?: () => void; onAddGroup?: () => void }

export function NodePalette({ manifests, sourceConnection, open, onOpenChange, onAddAction, onAddBinding, onAddAnnotation, onAddGroup }: PaletteProps) {
  const { t, i18n } = useTranslation()
  const [internalOpen, setInternalOpen] = useState(false)
  const [search, setSearch] = useState('')
  const expanded = open ?? internalOpen
  const setOpen = (value: boolean) => { if (open === undefined) setInternalOpen(value); onOpenChange?.(value) }
  const attachments = useMemo(() => {
    const values = new Map<ResourceType, string>()
    for (const manifest of manifests) for (const slot of manifest.bindingSlots) values.set(slot.resourceType, slot.name)
    return [...values]
  }, [manifests])
  const visible = useMemo(() => manifests.filter((manifest) => {
    const matchesSearch = manifestSearchText(manifest).includes(search.trim().toLowerCase())
    if (!matchesSearch || !sourceConnection) return matchesSearch
    const sourcePort = sourceConnection.manifest.outputPorts.find((port) => port.name === sourceConnection.handleId)
    return Boolean(sourcePort && manifest.inputPorts.some((port) => port.kind === sourcePort.kind))
  }), [manifests, search, sourceConnection])
  const allGroups = useMemo(() => groupManifests(manifests), [manifests])
  const visibleGroups = useMemo(() => groupManifests(visible), [visible])
  const drag = (event: DragEvent, payload: object) => { event.dataTransfer.setData('application/agentx-studio', JSON.stringify(payload)); event.dataTransfer.effectAllowed = 'copy' }
  useEffect(() => { if (expanded) document.querySelector<HTMLInputElement>('[data-testid="node-creator"] input')?.focus() }, [expanded])
  const handleCreatorKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (!['ArrowDown', 'ArrowUp', 'Enter'].includes(event.key)) return
    const items = [...event.currentTarget.querySelectorAll<HTMLButtonElement>('[data-node-creator-item]')]
    if (!items.length) return
    const current = document.activeElement instanceof HTMLButtonElement ? items.indexOf(document.activeElement) : -1
    if (event.key === 'Enter' && current >= 0) { event.preventDefault(); items[current].click(); return }
    event.preventDefault()
    items[event.key === 'ArrowDown' ? (current + 1) % items.length : (current - 1 + items.length) % items.length].focus()
  }
  return <aside className="relative w-14 shrink-0 border-r border-border bg-surface" data-testid="node-creator-shell">
    <div className="flex h-full flex-col items-center" data-testid="node-creator-rail">
      <div className="flex shrink-0 flex-col items-center gap-1.5 border-b border-border py-2.5">
        <RailButton icon={<Search className="size-4" />} label={t('studio.palette.search')} onClick={() => setOpen(true)} />
        <RailButton icon={<FileText className="size-4" />} label={t('studio.palette.note')} onClick={onAddAnnotation} />
        <RailButton icon={<Layers3 className="size-4" />} label={t('studio.palette.group')} onClick={onAddGroup} />
      </div>
      <div className="min-h-0 w-full flex-1 overflow-y-auto overflow-x-hidden py-2">
        {[...allGroups.entries()].map(([category, items]) => <section className="mb-2" key={category}><div className="mx-auto mb-1 h-px w-5 bg-border" />{items.map((manifest) => { const localized = localizeManifest(manifest, i18n.language); return <Tooltip content={`${localized.displayName} · ${localized.description}`} key={`${manifest.nodeType}@${manifest.version}`}><button aria-label={localized.displayName} className="mx-auto grid size-10 place-items-center text-primary hover:bg-muted" data-testid={`rail-action-${manifest.nodeType}`} draggable onClick={() => onAddAction(manifest)} onDragStart={(event) => drag(event, { kind: 'action', nodeType: manifest.nodeType, version: manifest.version })} type="button"><NodePreview icon={manifest.iconKey} role={canvasNodeRole(manifest)} size="sm" /></button></Tooltip>})}</section>)}
        <div className="mx-auto mb-1 h-px w-5 bg-border" />
        {attachments.map(([resourceType, role]) => <Tooltip content={t(`resourceTypes.${resourceType}`)} key={resourceType}><button aria-label={t(`resourceTypes.${resourceType}`)} className="mx-auto grid size-10 place-items-center hover:bg-muted" draggable onClick={() => onAddBinding(resourceType, role)} onDragStart={(event) => drag(event, { kind: 'binding', resourceType, role })} type="button"><span className="grid size-7 place-items-center rounded-full border border-warning/50 bg-warning/10 text-warning"><NodeIcon className="size-3.5" iconKey={resourceIcon(resourceType)} /></span></button></Tooltip>)}
      </div>
    </div>
    {expanded && <div className="absolute inset-y-0 left-full z-30 flex w-80 flex-col border-r border-border bg-surface shadow-xl" data-testid="node-creator" onKeyDown={handleCreatorKeyDown}>
      <div className="flex items-center gap-2 border-b border-border p-3"><label className="relative min-w-0 flex-1"><Search className="pointer-events-none absolute left-2.5 top-2.5 size-3.5 text-muted-foreground" /><Input aria-label={t('studio.palette.search')} className="h-9 bg-canvas pl-8 text-xs" onChange={(event) => setSearch(event.target.value)} placeholder={t('studio.palette.search')} value={search} /></label><button aria-label={t('studio.close')} className="grid size-8 place-items-center rounded-md text-muted-foreground hover:bg-muted" onClick={() => setOpen(false)} type="button"><X className="size-4" /></button></div>
      <div className="flex gap-1 border-b border-border px-3 py-2"><button className="flex items-center gap-1 rounded-md px-2 py-1.5 text-[10px] text-muted-foreground hover:bg-muted" onClick={onAddAnnotation} type="button"><FileText className="size-3.5" />{t('studio.palette.note')}</button><button className="flex items-center gap-1 rounded-md px-2 py-1.5 text-[10px] text-muted-foreground hover:bg-muted" onClick={onAddGroup} type="button"><Layers3 className="size-3.5" />{t('studio.palette.group')}</button></div>
      <div className="min-h-0 flex-1 overflow-y-auto p-3">{[...visibleGroups.entries()].map(([category, items]) => <section className="mb-5" key={category}><PaletteHeading count={items.length}>{categoryLabel(category, t)}</PaletteHeading><div className="mt-2 space-y-1">{items.map((manifest) => { const localized = localizeManifest(manifest, i18n.language); return <PaletteItem data-node-creator-item data-testid={`palette-action-${manifest.nodeType}`} draggable icon={manifest.iconKey} key={`${manifest.nodeType}@${manifest.version}`} label={localized.displayName} meta={localized.description || manifest.capability} onClick={() => { onAddAction(manifest); setOpen(false) }} onDragStart={(event) => drag(event, { kind: 'action', nodeType: manifest.nodeType, version: manifest.version })} role={canvasNodeRole(manifest)} /> })}</div></section>)}{!sourceConnection && <section><PaletteHeading count={attachments.length}>{t('studio.palette.attachments')}</PaletteHeading><div className="mt-2 space-y-1">{attachments.map(([resourceType, role]) => <PaletteItem data-node-creator-item data-testid={`palette-binding-${resourceType}`} draggable icon={resourceIcon(resourceType)} key={resourceType} label={t(`resourceTypes.${resourceType}`)} meta={role.replaceAll('_', ' ')} onClick={() => { onAddBinding(resourceType, role); setOpen(false) }} onDragStart={(event) => drag(event, { kind: 'binding', resourceType, role })} role="default" />)}</div></section>}</div>
    </div>}
  </aside>
}

function groupManifests(manifests: NodeManifest[]) { const groups = new Map<NodeCategory, NodeManifest[]>(); for (const manifest of manifests) { const category = nodeCategory(manifest); groups.set(category, [...(groups.get(category) ?? []), manifest]) } return groups }
function RailButton({ icon, label, onClick }: { icon: React.ReactNode; label: string; onClick?: () => void }) { return <Tooltip content={label}><button aria-label={label} className="grid size-9 place-items-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground" onClick={onClick} type="button">{icon}</button></Tooltip> }
function PaletteHeading({ children, count }: { children: string; count: number }) { return <h2 className="flex items-center gap-1.5 text-[10px] font-semibold uppercase text-muted-foreground"><span className="size-1.5 rounded-full bg-primary/70" />{children}<span className="ml-auto rounded bg-muted px-1.5 py-0.5 text-[9px] font-medium">{count}</span></h2> }
function PaletteItem({ icon, label, meta, role, ...props }: { icon: string; label: string; meta: string; role: CanvasNodeRole } & React.ButtonHTMLAttributes<HTMLButtonElement>) { return <button className="group flex w-full items-center gap-3 rounded-md border border-transparent px-2 py-2.5 text-left hover:border-border hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30" type="button" {...props}><NodePreview icon={icon} role={role} /><span className="min-w-0"><strong className="block truncate text-xs font-medium">{label}</strong><span className="mt-0.5 block line-clamp-2 text-[10px] leading-4 text-muted-foreground">{meta}</span></span></button> }
function NodePreview({ icon, role, size = 'md' }: { icon: string; role: CanvasNodeRole; size?: 'sm' | 'md' }) { return <span className={`studio-node studio-node-${role} relative grid shrink-0 place-items-center ${size === 'sm' ? 'size-7' : 'size-12'}`}><span className="studio-node-surface absolute inset-0" /><NodeIcon className={`relative z-10 ${size === 'sm' ? 'size-3.5' : 'size-5'}`} iconKey={icon} /></span> }
