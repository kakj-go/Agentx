import { Search } from 'lucide-react'
import { useMemo, useState, type DragEvent } from 'react'
import { useTranslation } from 'react-i18next'

import { Input } from '../../../shared/ui/input'
import type { NodeManifest, ResourceType } from '../model/types'
import { NodeIcon } from '../nodes/node-icon'

export function NodePalette({ manifests, onAddAction, onAddBinding }: { manifests: NodeManifest[]; onAddAction: (manifest: NodeManifest) => void; onAddBinding: (resourceType: ResourceType, role: string) => void }) {
  const { t } = useTranslation()
  const [search, setSearch] = useState('')
  const visible = useMemo(() => manifests.filter((manifest) => `${manifest.displayName} ${manifest.nodeType} ${manifest.keywords.join(' ')}`.toLowerCase().includes(search.toLowerCase())), [manifests, search])
  const attachments = useMemo(() => {
    const values = new Map<ResourceType, string>()
    for (const manifest of manifests) for (const slot of manifest.bindingSlots) values.set(slot.resourceType, slot.name)
    return [...values]
  }, [manifests])
  const drag = (event: DragEvent, payload: object) => { event.dataTransfer.setData('application/agentx-studio', JSON.stringify(payload)); event.dataTransfer.effectAllowed = 'copy' }
  return <aside className="flex w-64 shrink-0 flex-col border-r border-border bg-surface">
    <div className="border-b border-border p-3"><label className="relative block"><Search className="pointer-events-none absolute left-2.5 top-2.5 size-3.5 text-muted-foreground" /><Input aria-label={t('studio.palette.search')} className="h-9 pl-8 text-xs" onChange={(event) => setSearch(event.target.value)} placeholder={t('studio.palette.search')} value={search} /></label></div>
    <div className="min-h-0 flex-1 overflow-y-auto p-3"><PaletteHeading>{t('studio.palette.actionNodes')}</PaletteHeading><div className="mt-2 space-y-1">{visible.map((manifest) => <PaletteItem data-testid={`palette-action-${manifest.nodeType}`} draggable icon={manifest.iconKey} key={`${manifest.nodeType}@${manifest.version}`} label={manifest.displayName} meta={manifest.category} onClick={() => onAddAction(manifest)} onDragStart={(event) => drag(event, { kind: 'action', nodeType: manifest.nodeType, version: manifest.version })} />)}</div>
      <PaletteHeading className="mt-5">{t('studio.palette.attachments')}</PaletteHeading><div className="mt-2 space-y-1">{attachments.map(([resourceType, role]) => <PaletteItem data-testid={`palette-binding-${resourceType}`} draggable icon={attachmentIcon(resourceType)} key={resourceType} label={t(`resourceTypes.${resourceType}`)} meta={t('studio.palette.ai')} onClick={() => onAddBinding(resourceType, role)} onDragStart={(event) => drag(event, { kind: 'binding', resourceType, role })} />)}</div>
    </div>
  </aside>
}

function PaletteHeading({ children, className = '' }: { children: string; className?: string }) { return <h2 className={`text-[10px] font-semibold uppercase text-muted-foreground ${className}`}>{children}</h2> }
function PaletteItem({ icon, label, meta, ...props }: { icon: string; label: string; meta: string } & React.ButtonHTMLAttributes<HTMLButtonElement>) { return <button className="flex w-full items-center gap-2 border border-transparent px-2 py-2 text-left hover:border-border hover:bg-muted/50" type="button" {...props}><span className="grid size-7 place-items-center rounded-md bg-primary/10 text-primary"><NodeIcon className="size-3.5" iconKey={icon} /></span><span className="min-w-0"><strong className="block truncate text-[11px] font-medium">{label}</strong><span className="block text-[9px] text-muted-foreground">{meta}</span></span></button> }
const attachmentIcon = (type: ResourceType) => ({ model: 'brain-circuit', mcp_tool: 'wrench', memory: 'memory-stick', rag: 'database', skill: 'sparkles' } as Partial<Record<ResourceType, string>>)[type] ?? 'box'
