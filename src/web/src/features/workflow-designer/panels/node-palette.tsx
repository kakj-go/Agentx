import { ArrowLeft, ChevronDown, FileText, Layers3, LogOut, PanelLeftClose, Plus, Search } from 'lucide-react'
import { useEffect, useMemo, useState, type DragEvent, type KeyboardEvent } from 'react'
import { useTranslation } from 'react-i18next'

import { Input } from '../../../shared/ui/input'
import { Tooltip } from '../../../shared/ui/tooltip'
import { localizeManifest, manifestSearchText } from '../model/manifest-localization'
import type { NodeManifest } from '../model/types'
import { groupColor, nodeGroup, type NodeVisualGroup } from '../nodes/node-appearance'
import { NodeIcon } from '../nodes/node-icon'

type PaletteProps = { manifests: NodeManifest[]; sourceConnection?: { manifest: NodeManifest; handleId: string; manifestPortName?: string }; collapsed?: boolean; onCollapsedChange?: (collapsed: boolean) => void; onAddAction: (manifest: NodeManifest, targetHandle?: string) => void; onAddExit?: () => void; onAddAnnotation?: () => void; onAddGroup?: () => void }

/** Dify-style visual groups driving both the palette sections and the card icon colors. */
const GROUP_ORDER: NodeVisualGroup[] = ['start', 'ai', 'logic', 'transform', 'integrate', 'output']

export function NodePalette({ manifests, sourceConnection, collapsed, onCollapsedChange, onAddAction, onAddExit, onAddAnnotation, onAddGroup }: PaletteProps) {
  const { t, i18n } = useTranslation()
  const [internalCollapsed, setInternalCollapsed] = useState(false)
  const [search, setSearch] = useState('')
  const [portChoice, setPortChoice] = useState<NodeManifest>()
  const [openGroups, setOpenGroups] = useState<Set<NodeVisualGroup>>(new Set())
  const isCollapsed = collapsed ?? internalCollapsed
  const setCollapsed = (value: boolean) => {
    if (collapsed === undefined) setInternalCollapsed(value)
    onCollapsedChange?.(value)
  }
  const visible = useMemo(() => manifests.filter((manifest) => {
    const matchesSearch = manifestSearchText(manifest).includes(search.trim().toLowerCase())
    if (!matchesSearch || !sourceConnection) return matchesSearch
    const sourcePort = sourceOutputPort(sourceConnection)
    return Boolean(sourcePort && manifest.inputPorts.some((port) => port.kind === sourcePort.kind))
  }), [manifests, search, sourceConnection])
  const visibleGroups = useMemo(() => groupManifests(visible), [visible])
  const groupKeys = useMemo(() => GROUP_ORDER.filter((group) => visibleGroups.has(group)), [visibleGroups])
  const drag = (event: DragEvent, payload: object) => { event.dataTransfer.setData('application/agentx-studio', JSON.stringify(payload)); event.dataTransfer.effectAllowed = 'copy' }
  const addAction = (manifest: NodeManifest) => {
    const inputs = compatibleInputs(manifest, sourceConnection)
    if (inputs.length > 1) { setPortChoice(manifest); return }
    setPortChoice(undefined)
    onAddAction(manifest, inputs[0]?.name)
  }
  useEffect(() => {
    if (isCollapsed) return
    setOpenGroups((current) => current.size || !groupKeys[0] ? current : new Set([groupKeys[0]]))
    document.querySelector<HTMLInputElement>('[data-testid="node-creator"] input')?.focus()
  }, [groupKeys, isCollapsed])
  useEffect(() => {
    if (!sourceConnection) return
    setPortChoice(undefined)
    setSearch('')
  }, [sourceConnection])
  const handleCreatorKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (!['ArrowDown', 'ArrowUp', 'Enter'].includes(event.key)) return
    const items = [...event.currentTarget.querySelectorAll<HTMLButtonElement>('[data-node-creator-item]')]
    if (!items.length) return
    const current = document.activeElement instanceof HTMLButtonElement ? items.indexOf(document.activeElement) : -1
    if (event.key === 'Enter' && current >= 0) { event.preventDefault(); items[current].click(); return }
    event.preventDefault()
    items[event.key === 'ArrowDown' ? (current + 1) % items.length : (current - 1 + items.length) % items.length].focus()
  }
  const groupsAutoOpen = Boolean(search.trim()) || Boolean(sourceConnection)
  const sourceKind = sourceOutputPort(sourceConnection)?.kind
  const canAddExit = Boolean(onAddExit && (!sourceConnection || sourceKind === 'main' || sourceKind === 'error'))
  const isGroupOpen = (key: NodeVisualGroup) => groupsAutoOpen || openGroups.has(key)
  const toggleGroup = (key: NodeVisualGroup) => setOpenGroups((current) => {
    const next = new Set(current)
    if (next.has(key)) next.delete(key)
    else next.add(key)
    return next
  })
  return <aside className={`flex h-full shrink-0 flex-col border-r border-border bg-surface transition-[width] ${isCollapsed ? 'w-12' : 'w-[264px]'}`} data-testid="node-creator-shell">
    {isCollapsed ? (
      <div className="flex h-full w-12 flex-col items-center gap-2 py-3">
        <Tooltip content={t('studio.palette.expand')}>
          <button aria-label={t('studio.palette.expand')} className="grid size-10 place-items-center rounded-full border border-border bg-surface text-foreground shadow-sm transition-colors hover:border-primary hover:bg-muted hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30" data-testid="node-creator-trigger" onClick={() => setCollapsed(false)} type="button"><Plus className="size-5" /></button>
        </Tooltip>
        <div className="my-1 h-px w-6 bg-border" />
        {onAddAnnotation && <RailTool aria-label={t('studio.palette.note')} icon={<FileText className="size-4" />} onClick={() => onAddAnnotation()} />}
        {onAddGroup && <RailTool aria-label={t('studio.palette.group')} icon={<Layers3 className="size-4" />} onClick={() => onAddGroup()} />}
        {canAddExit && <RailTool aria-label={t('studio.palette.exit')} data-testid="palette-exit" icon={<LogOut className="size-4" />} onClick={() => onAddExit?.()} />}
      </div>
    ) : (
      <div className="flex min-h-0 flex-1 flex-col" data-testid="node-creator" onKeyDown={handleCreatorKeyDown}>
        <div className="flex items-center gap-2 px-3 pb-2 pt-3">
          <h2 className="min-w-0 flex-1 truncate text-[13px] font-semibold leading-none">{t('studio.palette.title')}</h2>
          <button aria-label={t('studio.palette.collapse')} className="grid size-7 place-items-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground" onClick={() => setCollapsed(true)} type="button"><PanelLeftClose className="size-4" /></button>
        </div>
        <div className="px-3 pb-2"><label className="relative block"><Search className="pointer-events-none absolute left-2.5 top-2.5 size-3.5 text-muted-foreground" /><Input aria-label={t('studio.palette.search')} className="h-9 bg-canvas pl-8 text-xs" onChange={(event) => setSearch(event.target.value)} placeholder={t('studio.palette.search')} value={search} /></label></div>
        <div className="flex gap-1 border-b border-border px-3 py-2">
          {onAddAnnotation && <ToolButton icon={<FileText className="size-3.5" />} label={t('studio.palette.note')} onClick={() => onAddAnnotation()} />}
          {onAddGroup && <ToolButton icon={<Layers3 className="size-3.5" />} label={t('studio.palette.group')} onClick={() => onAddGroup()} />}
          {canAddExit && <ToolButton data-testid="palette-exit" icon={<LogOut className="size-3.5" />} label={t('studio.palette.exit')} onClick={() => onAddExit?.()} />}
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">{portChoice ? <div className="p-1"><PortChoice manifest={portChoice} onBack={() => setPortChoice(undefined)} onChoose={(targetHandle) => { onAddAction(portChoice, targetHandle); setPortChoice(undefined) }} sourceConnection={sourceConnection!} /></div> : GROUP_ORDER.filter((group) => visibleGroups.has(group)).map((group) => <PaletteSection count={visibleGroups.get(group)!.length} groupKey={group} key={group} label={t(`studio.palette.categories.${group}`)} onToggle={() => toggleGroup(group)} open={isGroupOpen(group)}>{visibleGroups.get(group)!.map((manifest) => { const localized = localizeManifest(manifest, i18n.language); return <PaletteItem data-node-creator-item data-testid={`palette-action-${manifest.nodeType}`} draggable={!sourceConnection} iconKey={manifest.iconKey} key={`${manifest.nodeType}@${manifest.version}`} label={localized.displayName} meta={localized.description || manifest.capability} nodeType={manifest.nodeType} onClick={() => addAction(manifest)} onDragStart={(event) => drag(event, { kind: 'action', nodeType: manifest.nodeType, version: manifest.version })} /> })}</PaletteSection>)}</div>
      </div>
    )}
  </aside>
}

function RailTool({ 'aria-label': ariaLabel, icon, onClick, ...rest }: { 'aria-label': string; icon: React.ReactNode; onClick: () => void } & React.ButtonHTMLAttributes<HTMLButtonElement>) {
  return <button aria-label={ariaLabel} className="grid size-9 place-items-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground" onClick={onClick} type="button" {...rest}>{icon}</button>
}

function ToolButton({ icon, label, onClick, ...rest }: { icon: React.ReactNode; label: string; onClick: () => void } & React.ButtonHTMLAttributes<HTMLButtonElement>) {
  return <button className="flex items-center gap-1 rounded-md px-2 py-1.5 text-[10px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground" onClick={onClick} type="button" {...rest}>{icon}{label}</button>
}

function groupManifests(manifests: NodeManifest[]) {
  const groups = new Map<NodeVisualGroup, NodeManifest[]>()
  for (const manifest of manifests) {
    const group = nodeGroup(manifest.nodeType)
    groups.set(group, [...(groups.get(group) ?? []), manifest])
  }
  return groups
}
function sourceOutputPort(sourceConnection?: PaletteProps['sourceConnection']) { return sourceConnection?.manifest.outputPorts.find((port) => port.name === (sourceConnection.manifestPortName ?? sourceConnection.handleId)) }
function compatibleInputs(manifest: NodeManifest, sourceConnection?: PaletteProps['sourceConnection']) { const source = sourceOutputPort(sourceConnection); return source ? manifest.inputPorts.filter((port) => port.kind === source.kind) : [] }
function PortChoice({ manifest, sourceConnection, onBack, onChoose }: { manifest: NodeManifest; sourceConnection: NonNullable<PaletteProps['sourceConnection']>; onBack: () => void; onChoose: (handle: string) => void }) { const { t, i18n } = useTranslation(); const localized = localizeManifest(manifest, i18n.language); const inputs = compatibleInputs(manifest, sourceConnection); return <section data-testid="node-port-choice"><button className="mb-3 flex h-8 items-center gap-2 rounded-md px-2 text-xs text-muted-foreground hover:bg-muted" onClick={onBack} type="button"><ArrowLeft className="size-3.5" />{t('studio.palette.back')}</button><h2 className="text-xs font-semibold">{t('studio.palette.chooseInput', { node: localized.displayName })}</h2><div className="mt-3 space-y-1">{inputs.map((port) => <button className="flex w-full items-center justify-between rounded-md border border-border px-3 py-2.5 text-left text-xs hover:border-primary hover:bg-primary/5" data-node-creator-item data-testid={`port-choice-${port.name}`} key={port.name} onClick={() => onChoose(port.name)} type="button"><span>{localized.inputPortLabel(port.name)}</span><span className="text-[9px] text-muted-foreground">{port.name}</span></button>)}</div></section> }
function PaletteSection({ groupKey, label, count, open, onToggle, children }: { groupKey: NodeVisualGroup; label: string; count: number; open: boolean; onToggle: () => void; children: React.ReactNode }) { return <section className="border-b border-border last:border-b-0"><button aria-expanded={open} className="flex w-full items-center gap-2 px-3 py-3 text-left text-[10px] font-semibold uppercase text-muted-foreground hover:bg-muted/50" data-testid={`palette-group-${groupKey}`} onClick={onToggle} type="button"><span className="size-1.5 rounded-full" style={{ backgroundColor: groupColor(groupKey) }} /><span className="min-w-0 flex-1 truncate">{label}</span><span className="rounded bg-muted px-1.5 py-0.5 text-[9px] font-medium">{count}</span><ChevronDown className={`size-3.5 transition-transform ${open ? 'rotate-180' : ''}`} /></button>{open && <div className="space-y-1 px-2 pb-3" data-testid={`palette-group-content-${groupKey}`}>{children}</div>}</section> }
function PaletteItem({ iconKey, label, meta, nodeType, ...props }: { iconKey: string; label: string; meta: string; nodeType: string } & React.ButtonHTMLAttributes<HTMLButtonElement>) {
  const tint = groupColor(nodeGroup(nodeType))
  return <button className="group flex w-full items-start gap-2.5 rounded-lg border border-transparent px-2 py-2 text-left transition-colors hover:border-border hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30" type="button" {...props}><span className="grid size-6 shrink-0 place-items-center rounded-md text-white" style={{ backgroundColor: tint }}><NodeIcon className="size-3.5" iconKey={iconKey} /></span><span className="min-w-0"><strong className="block truncate text-xs font-medium">{label}</strong><span className="mt-0.5 block line-clamp-2 text-[10px] leading-4 text-muted-foreground">{meta}</span></span></button>
}
