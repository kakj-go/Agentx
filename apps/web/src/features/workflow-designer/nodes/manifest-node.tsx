import { Handle, Position, useUpdateNodeInternals, type NodeProps } from '@xyflow/react'
import { AlertTriangle, Plus, Star } from 'lucide-react'
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import { cn } from '../../../shared/lib/cn'
import { localizedNodeLabel, localizeManifest } from '../model/manifest-localization'
import type { CanvasNodeRole, NodeManifest, PortKind, StudioNode } from '../model/types'
import { canvasNodeMetrics, canvasNodeRole } from './node-appearance'
import { NodeIcon } from './node-icon'

export type NodeBindingSummary = { role: string; resourceType: string; label: string }

export function ManifestNode({ id, data, selected, manifest, runtimeStatus, zoom = 1, primary = false, onQuickAdd, bindingSummaries = [] }: NodeProps<StudioNode> & { manifest?: NodeManifest; runtimeStatus?: string; zoom?: number; primary?: boolean; onQuickAdd?: (nodeId: string, handleId: string) => void; bindingSummaries?: NodeBindingSummary[] }) {
  const { t, i18n } = useTranslation()
  const updateNodeInternals = useUpdateNodeInternals()
  useEffect(() => updateNodeInternals(id), [bindingSummaries, data.label, id, updateNodeInternals])
  if (data.editorKind !== 'action') return null
  const role = canvasNodeRole(manifest)
  const metrics = canvasNodeMetrics(role, { inputs: manifest?.inputPorts.length, outputs: manifest?.outputPorts.length, bindings: manifest?.bindingSlots.length })
  const localized = manifest ? localizeManifest(manifest, i18n.language) : undefined
  const statusClass = selected
    ? 'studio-node-selected'
    : runtimeStatus === 'running' ? 'studio-node-running'
      : runtimeStatus === 'succeeded' ? 'studio-node-succeeded'
        : runtimeStatus === 'failed' ? 'studio-node-failed' : undefined
  const label = manifest ? localizedNodeLabel(manifest, data.label, i18n.language) : data.label || data.nodeType
  return <div className={cn('studio-node relative shrink-0', `studio-node-${role}`, zoom < 0.65 && 'studio-node-low-zoom')} data-role={role} data-testid={`studio-node-${id}`} style={{ width: metrics.width, height: metrics.height }}>
    {manifest?.inputPorts.map((port, index) => <PortHandle id={port.name} key={`in-${port.name}`} kind={port.kind} label={localized?.inputPortLabel(port.name) ?? port.name} placement={portPlacement(role, 'input', port.kind, port.name, index, manifest.inputPorts.length)} type="target" />)}
    <NodeSurface bindingSummaries={bindingSummaries} data={data} label={label} localizedDescription={localized?.description} manifest={manifest} role={role} statusClass={statusClass} />
    {metrics.labelBelow && <NodeLabel label={label} subtitle={localized?.displayName ?? data.nodeType} />}
    {manifest?.outputPorts.map((port, index) => <PortHandle addLabel={t('studio.ports.addAfter', { label: localized?.outputPortLabel(port.name) ?? port.name })} id={port.name} key={`out-${port.name}`} kind={port.kind} label={localized?.outputPortLabel(port.name) ?? port.name} onQuickAdd={onQuickAdd ? () => onQuickAdd(id, port.name) : undefined} placement={portPlacement(role, 'output', port.kind, port.name, index, manifest.outputPorts.length)} type="source" />)}
    {manifest?.bindingSlots.map((slot, index) => <PortHandle id={`binding:${slot.name}`} key={slot.name} kind="binding" label={localized?.bindingSlotLabel(slot.name) ?? slot.name} placement={{ position: Position.Bottom, axis: ((index + 1) / (manifest.bindingSlots.length + 1)) * 100 }} type="target" />)}
    {primary && <span className="absolute -left-2 -top-2 z-30 grid size-5 place-items-center rounded-full bg-primary text-primary-foreground shadow" title={t('studio.primaryOutput')}><Star className="size-3 fill-current" /></span>}
    {data.disabled && <AlertTriangle className="absolute -right-2 -top-2 z-30 size-5 rounded-full bg-surface p-0.5 text-warning shadow" />}
  </div>
}

function NodeSurface({ data, label, manifest, role, bindingSummaries, localizedDescription, statusClass }: { data: Extract<StudioNode['data'], { editorKind: 'action' }>; label: string; manifest?: NodeManifest; role: CanvasNodeRole; bindingSummaries: NodeBindingSummary[]; localizedDescription?: string; statusClass?: string }) {
  const { t } = useTranslation()
  const rich = role === 'agent' || role === 'suspend' || role === 'approval' || role === 'error_handler'
  return <div className={cn('studio-node-surface relative grid size-full place-items-center overflow-hidden', rich ? 'p-3' : 'p-2', statusClass)}>
    <span className={cn('studio-node-icon grid place-items-center text-primary', role === 'trigger' ? 'rounded-full bg-success/12 text-success' : role === 'branch' || role === 'loop' ? 'rounded-lg bg-warning/15 text-warning' : role === 'code' || role === 'error_handler' ? 'rounded-md bg-danger/10 text-danger' : 'rounded-xl bg-primary/10')}><NodeIcon className={rich ? 'size-7' : 'size-8'} iconKey={manifest?.iconKey ?? roleIcon(role)} /></span>
    {rich && <div className="mt-1 min-w-0 max-w-full text-center"><strong className="block truncate text-xs font-semibold">{label}</strong><span className="mt-0.5 block truncate text-[10px] text-muted-foreground">{localizedDescription || manifest?.displayName || data.nodeType}</span>{role === 'agent' && <div className="mt-2 flex flex-wrap justify-center gap-1">{bindingSummaries.length ? bindingSummaries.slice(0, 5).map((binding) => <span className="max-w-28 truncate rounded bg-warning/10 px-1.5 py-0.5 text-[8px] text-warning" key={`${binding.role}-${binding.label}`}>{binding.label}</span>) : <span className="text-[8px] text-muted-foreground">{t('studio.node.noBindings')}</span>}</div>}</div>}
  </div>
}

function NodeLabel({ label, subtitle }: { label: string; subtitle: string }) { return <div className="studio-node-label pointer-events-none absolute left-1/2 top-full z-10 mt-2 w-36 -translate-x-1/2 text-center"><strong className="block line-clamp-2 text-[11px] font-semibold leading-4">{label}</strong><span className="block truncate text-[9px] leading-3 text-muted-foreground">{subtitle}</span></div> }

type Placement = { position: Position; axis: number }

function PortHandle({ id, kind, label, placement, type, onQuickAdd, addLabel }: { id: string; kind: PortKind | 'binding'; label: string; placement: Placement; type: 'source' | 'target'; onQuickAdd?: () => void; addLabel?: string }) {
  const vertical = placement.position === Position.Left || placement.position === Position.Right
  const handleStyle = vertical ? { top: `${placement.axis}%` } : { left: `${placement.axis}%` }
  const labelClass = placement.position === Position.Left ? 'right-full mr-2 -translate-y-1/2' : placement.position === Position.Right ? 'left-full ml-2 -translate-y-1/2' : placement.position === Position.Bottom ? 'top-full mt-2 -translate-x-1/2' : 'bottom-full mb-2 -translate-x-1/2'
  const labelStyle = vertical ? { top: `${placement.axis}%` } : { left: `${placement.axis}%` }
  return <>
    <Handle className={cn('studio-handle !absolute !z-30 !size-3 !border-2 !border-background shadow-sm', kind === 'binding' ? '!rotate-45 !rounded-[2px] !bg-warning' : kind === 'error' ? '!bg-danger' : type === 'target' ? '!bg-muted-foreground' : '!bg-primary')} id={id} position={placement.position} style={handleStyle} title={label} type={type} />
    <span className={cn('studio-port-label pointer-events-none absolute z-20 max-w-20 truncate text-[8px] font-medium text-muted-foreground', labelClass)} style={labelStyle}>{label}</span>
    {onQuickAdd && <button aria-label={addLabel} className={cn('studio-port-add nodrag absolute z-40 grid size-5 place-items-center rounded-full border border-border bg-surface text-primary shadow-sm hover:bg-primary hover:text-primary-foreground', placement.position === Position.Bottom ? 'top-full mt-5 -translate-x-1/2' : 'left-full ml-6 -translate-y-1/2')} onClick={(event) => { event.stopPropagation(); onQuickAdd() }} style={labelStyle} title={addLabel} type="button"><Plus className="size-3" /></button>}
  </>
}

function portPlacement(role: CanvasNodeRole, direction: 'input' | 'output', kind: PortKind, name: string, index: number, count: number): Placement {
  if (direction === 'input') return { position: Position.Left, axis: ((index + 1) / (count + 1)) * 100 }
  if (kind === 'error' || name === 'error') return { position: Position.Bottom, axis: role === 'agent' ? 90 : 50 }
  return { position: Position.Right, axis: ((index + 1) / (count + 1)) * 100 }
}

function roleIcon(role: CanvasNodeRole) {
  return ({ trigger: 'mouse-pointer-click', branch: 'split', merge: 'git-merge', loop: 'repeat-2', agent: 'bot', code: 'code-2', suspend: 'clock-3', approval: 'badge-check', sub_workflow: 'git-merge', error_handler: 'triangle-alert' } as Partial<Record<CanvasNodeRole, string>>)[role] ?? 'box'
}

export function AttachmentNode({ id, data, selected }: NodeProps<StudioNode>) {
  const { t } = useTranslation()
  if (data.editorKind !== 'binding') return null
  const metrics = canvasNodeMetrics('default', { kind: 'binding' })
  return <div className={cn('studio-node-attachment relative flex shrink-0 flex-col items-center justify-center bg-surface shadow-sm', selected && 'studio-node-selected')} data-testid={`studio-node-${id}`} style={{ width: metrics.width, height: metrics.height }}>
    <span className="grid size-9 place-items-center rounded-full bg-warning/10 text-warning"><NodeIcon className="size-4" iconKey={attachmentIcon(data.resourceType)} /></span>
    <span className="mt-1 max-w-16 truncate text-[9px] font-medium">{data.resourceName ?? data.label}</span>
    <span className="max-w-16 truncate text-[8px] text-muted-foreground">{t(`resourceTypes.${data.resourceType}`)}</span>
    <Handle className="!top-[-6px] !size-3 !border-2 !border-background !bg-warning" id="resource" position={Position.Top} type="source" />
  </div>
}

function attachmentIcon(resourceType: string) { return ({ model: 'brain-circuit', mcp_tool: 'wrench', memory: 'memory-stick', rag: 'database', skill: 'sparkles' } as Record<string, string>)[resourceType] ?? 'box' }
