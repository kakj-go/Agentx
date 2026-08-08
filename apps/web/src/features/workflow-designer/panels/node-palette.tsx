import { ArrowLeft, ChevronDown, FileText, Layers3, Plus, Search, X } from 'lucide-react'
import { useEffect, useMemo, useState, type DragEvent, type KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'

import { Input } from '../../../shared/ui/input'
import { Tooltip } from '../../../shared/ui/tooltip'
import { localizeManifest, manifestSearchText } from '../model/manifest-localization'
import type { BindingSlot, CanvasNodeRole, NodeManifest, ResourceType } from '../model/types'
import { categoryLabel, canvasNodeFamily, canvasNodeRole, nodeCategory, resourceIcon, type NodeCategory } from '../nodes/node-appearance'
import { NodeIcon } from '../nodes/node-icon'

type PaletteProps = { manifests: NodeManifest[]; sourceConnection?: { manifest: NodeManifest; handleId: string }; bindingSlot?: BindingSlot; open?: boolean; onOpenChange?: (open: boolean) => void; onAddAction: (manifest: NodeManifest, targetHandle?: string) => void; onAddBinding: (resourceType: ResourceType, role: string) => void; onAddAnnotation?: () => void; onAddGroup?: () => void }
type PaletteGroupKey = NodeCategory | 'attachments'

export function NodePalette({ manifests, sourceConnection, bindingSlot, open, onOpenChange, onAddAction, onAddBinding, onAddAnnotation, onAddGroup }: PaletteProps) {
  const { t, i18n } = useTranslation()
  const [internalOpen, setInternalOpen] = useState(false)
  const [search, setSearch] = useState('')
  const [portChoice, setPortChoice] = useState<NodeManifest>()
  const [openGroups, setOpenGroups] = useState<Set<PaletteGroupKey>>(new Set())
  const expanded = open ?? internalOpen
  const attachments = useMemo(() => {
    const values = new Map<ResourceType, string>()
    for (const manifest of manifests) for (const slot of manifest.bindingSlots) values.set(slot.resourceType, slot.name)
    const result = [...values]
    return bindingSlot ? result.filter(([resourceType]) => resourceType === bindingSlot.resourceType).map(([resourceType]) => [resourceType, bindingSlot.name] as [ResourceType, string]) : result
  }, [bindingSlot, manifests])
  const visible = useMemo(() => manifests.filter((manifest) => {
    if (bindingSlot) return false
    const matchesSearch = manifestSearchText(manifest).includes(search.trim().toLowerCase())
    if (!matchesSearch || !sourceConnection) return matchesSearch
    const sourcePort = sourceConnection.manifest.outputPorts.find((port) => port.name === sourceConnection.handleId)
    return Boolean(sourcePort && manifest.inputPorts.some((port) => port.kind === sourcePort.kind))
  }), [bindingSlot, manifests, search, sourceConnection])
  const visibleGroups = useMemo(() => groupManifests(visible), [visible])
  const groupKeys = useMemo<PaletteGroupKey[]>(() => [...visibleGroups.keys(), ...(!sourceConnection && attachments.length ? ['attachments' as const] : [])], [attachments.length, sourceConnection, visibleGroups])
  const defaultGroup = groupKeys[0]
  const setOpen = (value: boolean) => {
    if (!value) {
      setPortChoice(undefined)
      setSearch('')
      setOpenGroups(new Set())
    }
    if (open === undefined) setInternalOpen(value)
    onOpenChange?.(value)
  }
  const drag = (event: DragEvent, payload: object) => { event.dataTransfer.setData('application/agentx-studio', JSON.stringify(payload)); event.dataTransfer.effectAllowed = 'copy' }
  const addAction = (manifest: NodeManifest) => {
    const inputs = compatibleInputs(manifest, sourceConnection)
    if (inputs.length > 1) { setPortChoice(manifest); return }
    onAddAction(manifest, inputs[0]?.name)
    setOpen(false)
  }
  useEffect(() => {
    if (!expanded) return
    setOpenGroups((current) => current.size || !defaultGroup ? current : new Set([defaultGroup]))
    document.querySelector<HTMLInputElement>('[data-testid="node-creator"] input')?.focus()
  }, [defaultGroup, expanded])
  const handleCreatorKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (!['ArrowDown', 'ArrowUp', 'Enter'].includes(event.key)) return
    const items = [...event.currentTarget.querySelectorAll<HTMLButtonElement>('[data-node-creator-item]')]
    if (!items.length) return
    const current = document.activeElement instanceof HTMLButtonElement ? items.indexOf(document.activeElement) : -1
    if (event.key === 'Enter' && current >= 0) { event.preventDefault(); items[current].click(); return }
    event.preventDefault()
    items[event.key === 'ArrowDown' ? (current + 1) % items.length : (current - 1 + items.length) % items.length].focus()
  }
  const searchActive = Boolean(search.trim())
  const isGroupOpen = (key: PaletteGroupKey) => searchActive || openGroups.has(key)
  const toggleGroup = (key: PaletteGroupKey) => setOpenGroups((current) => {
    const next = new Set(current)
    if (next.has(key)) next.delete(key)
    else next.add(key)
    return next
  })
  const runPaletteCommand = (command?: () => void) => { command?.(); setOpen(false) }
  return <aside className="pointer-events-none absolute inset-y-0 left-0 z-30" data-testid="node-creator-shell">
    <Tooltip content={t('studio.palette.search')}><button aria-label={t('studio.palette.search')} className="pointer-events-auto absolute left-4 top-4 grid size-10 place-items-center rounded-full border border-border bg-surface text-foreground shadow-md transition-colors hover:border-primary hover:bg-muted hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30" data-testid="node-creator-trigger" onClick={() => setOpen(true)} type="button"><Plus className="size-5" /></button></Tooltip>
    {expanded && <div className="pointer-events-auto absolute inset-y-0 left-16 z-30 flex w-80 flex-col border-x border-border bg-surface shadow-xl" data-testid="node-creator" onKeyDown={handleCreatorKeyDown}>
      <div className="flex items-center gap-2 border-b border-border p-3"><label className="relative min-w-0 flex-1"><Search className="pointer-events-none absolute left-2.5 top-2.5 size-3.5 text-muted-foreground" /><Input aria-label={t('studio.palette.search')} className="h-9 bg-canvas pl-8 text-xs" onChange={(event) => setSearch(event.target.value)} placeholder={t('studio.palette.search')} value={search} /></label><button aria-label={t('studio.close')} className="grid size-8 place-items-center rounded-md text-muted-foreground hover:bg-muted" onClick={() => setOpen(false)} type="button"><X className="size-4" /></button></div>
      <div className="flex gap-1 border-b border-border px-3 py-2"><button className="flex items-center gap-1 rounded-md px-2 py-1.5 text-[10px] text-muted-foreground hover:bg-muted" onClick={() => runPaletteCommand(onAddAnnotation)} type="button"><FileText className="size-3.5" />{t('studio.palette.note')}</button><button className="flex items-center gap-1 rounded-md px-2 py-1.5 text-[10px] text-muted-foreground hover:bg-muted" onClick={() => runPaletteCommand(onAddGroup)} type="button"><Layers3 className="size-3.5" />{t('studio.palette.group')}</button></div>
      <div className="min-h-0 flex-1 overflow-y-auto">{portChoice ? <div className="p-3"><PortChoice manifest={portChoice} onBack={() => setPortChoice(undefined)} onChoose={(targetHandle) => { onAddAction(portChoice, targetHandle); setOpen(false) }} sourceConnection={sourceConnection!} /></div> : <>{[...visibleGroups.entries()].map(([category, items]) => <PaletteSection count={items.length} groupKey={category} key={category} label={categoryLabel(category, t)} onToggle={() => toggleGroup(category)} open={isGroupOpen(category)}>{items.map((manifest) => { const localized = localizeManifest(manifest, i18n.language); return <PaletteItem data-node-creator-item data-testid={`palette-action-${manifest.nodeType}`} draggable={!sourceConnection} icon={manifest.iconKey} key={`${manifest.nodeType}@${manifest.version}`} label={localized.displayName} meta={localized.description || manifest.capability} onClick={() => addAction(manifest)} onDragStart={(event) => drag(event, { kind: 'action', nodeType: manifest.nodeType, version: manifest.version })} role={canvasNodeRole(manifest)} /> })}</PaletteSection>)}{!sourceConnection && attachments.length > 0 && <PaletteSection count={attachments.length} groupKey="attachments" label={t('studio.palette.attachments')} onToggle={() => toggleGroup('attachments')} open={isGroupOpen('attachments')}>{attachments.map(([resourceType, role]) => <PaletteItem data-node-creator-item data-testid={`palette-binding-${resourceType}`} draggable icon={resourceIcon(resourceType)} key={resourceType} label={t(`resourceTypes.${resourceType}`)} meta={role.replaceAll('_', ' ')} onClick={() => { onAddBinding(resourceType, role); setOpen(false) }} onDragStart={(event) => drag(event, { kind: 'binding', resourceType, role })} role="default" />)}</PaletteSection>}</>}</div>
    </div>}
  </aside>
}

function groupManifests(manifests: NodeManifest[]) { const groups = new Map<NodeCategory, NodeManifest[]>(); for (const manifest of manifests) { const category = nodeCategory(manifest); groups.set(category, [...(groups.get(category) ?? []), manifest]) } return groups }
function compatibleInputs(manifest: NodeManifest, sourceConnection?: PaletteProps['sourceConnection']) { const source = sourceConnection?.manifest.outputPorts.find((port) => port.name === sourceConnection.handleId); return source ? manifest.inputPorts.filter((port) => port.kind === source.kind) : [] }
function PortChoice({ manifest, sourceConnection, onBack, onChoose }: { manifest: NodeManifest; sourceConnection: NonNullable<PaletteProps['sourceConnection']>; onBack: () => void; onChoose: (handle: string) => void }) { const { t, i18n } = useTranslation(); const localized = localizeManifest(manifest, i18n.language); const inputs = compatibleInputs(manifest, sourceConnection); return <section data-testid="node-port-choice"><button className="mb-3 flex h-8 items-center gap-2 rounded-md px-2 text-xs text-muted-foreground hover:bg-muted" onClick={onBack} type="button"><ArrowLeft className="size-3.5" />{t('studio.palette.back')}</button><h2 className="text-xs font-semibold">{t('studio.palette.chooseInput', { node: localized.displayName })}</h2><div className="mt-3 space-y-1">{inputs.map((port) => <button className="flex w-full items-center justify-between rounded-md border border-border px-3 py-2.5 text-left text-xs hover:border-primary hover:bg-primary/5" data-node-creator-item data-testid={`port-choice-${port.name}`} key={port.name} onClick={() => onChoose(port.name)} type="button"><span>{localized.inputPortLabel(port.name)}</span><span className="text-[9px] text-muted-foreground">{port.name}</span></button>)}</div></section> }
function PaletteSection({ groupKey, label, count, open, onToggle, children }: { groupKey: PaletteGroupKey; label: string; count: number; open: boolean; onToggle: () => void; children: React.ReactNode }) { return <section className="border-b border-border last:border-b-0"><button aria-expanded={open} className="flex w-full items-center gap-2 px-3 py-3 text-left text-[10px] font-semibold uppercase text-muted-foreground hover:bg-muted/50" data-testid={`palette-group-${groupKey}`} onClick={onToggle} type="button"><span className="size-1.5 rounded-full bg-primary/70" /><span className="min-w-0 flex-1 truncate">{label}</span><span className="rounded bg-muted px-1.5 py-0.5 text-[9px] font-medium">{count}</span><ChevronDown className={`size-3.5 transition-transform ${open ? 'rotate-180' : ''}`} /></button>{open && <div className="space-y-1 px-3 pb-3" data-testid={`palette-group-content-${groupKey}`}>{children}</div>}</section> }
function PaletteItem({ icon, label, meta, role, ...props }: { icon: string; label: string; meta: string; role: CanvasNodeRole } & React.ButtonHTMLAttributes<HTMLButtonElement>) { return <button className="group flex w-full items-center gap-3 rounded-md border border-transparent px-2 py-2.5 text-left hover:border-border hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30" type="button" {...props}><NodePreview icon={icon} role={role} /><span className="min-w-0"><strong className="block truncate text-xs font-medium">{label}</strong><span className="mt-0.5 block line-clamp-2 text-[10px] leading-4 text-muted-foreground">{meta}</span></span></button> }
function NodePreview({ icon, role }: { icon: string; role: CanvasNodeRole }) { return <span className={`studio-node studio-node-${canvasNodeFamily(role)} studio-node-role-${role} relative grid size-12 shrink-0 place-items-center`}><span className="studio-node-surface absolute inset-0" /><NodeIcon className="relative z-10 size-5" iconKey={icon} /></span> }
