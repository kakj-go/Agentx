import { Handle, Position, useUpdateNodeInternals, type NodeProps } from '@xyflow/react'
import { AlertTriangle, Plus, Star, Zap } from 'lucide-react'
import { memo, useEffect, useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { cn } from '../../../shared/lib/cn'
import { localizedNodeLabel, localizeManifest } from '../model/manifest-localization'
import type { CanvasNodeFamily, CanvasNodeRole, NodeManifest, PortKind, ResourceType, StudioNode } from '../model/types'
import { useCanvasRenderStore } from '../store/canvas-render-store'
import type { NodeBindingSummary } from '../utils/graph-index'
import { canvasNodeFamily, canvasNodeMetrics, canvasNodeRole } from './node-appearance'
import { NodeIcon } from './node-icon'

export type { NodeBindingSummary } from '../utils/graph-index'

export const ManifestNode = memo(function ManifestNode({ id, data, selected }: NodeProps<StudioNode>) {
  const { t, i18n } = useTranslation()
  const manifestKey = data.editorKind === 'action' ? `${data.nodeType}@${data.typeVersion}` : ''
  const manifest = useCanvasRenderStore((state) => state.manifests.get(manifestKey))
  const runtimeStatus = useCanvasRenderStore((state) => state.runtimeStatuses.get(id))
  const bindingSummaries = useCanvasRenderStore((state) => state.bindingSummaries.get(id) ?? EMPTY_BINDINGS)
  const occupiedHandleSignature = useCanvasRenderStore((state) => state.occupiedHandlesByNodeId.get(id) ?? '')
  const primary = useCanvasRenderStore((state) => state.primaryOutputNodeId === id)
  const zoomTier = useCanvasRenderStore((state) => state.zoomTier)
  const onQuickAdd = useCanvasRenderStore((state) => state.onQuickAdd)
  const onSourceHover = useCanvasRenderStore((state) => state.onSourceHover)
  const updateNodeInternals = useUpdateNodeInternals()
  const portSignature = useMemo(() => manifest ? JSON.stringify([
    manifest.inputPorts.map((port) => [port.name, port.kind]),
    manifest.outputPorts.map((port) => [port.name, port.kind]),
    manifest.bindingSlots.map((slot) => [slot.name, slot.resourceType]),
  ]) : '', [manifest])
  const occupiedHandles = useMemo(() => new Set(occupiedHandleSignature ? occupiedHandleSignature.split('\u0001') : []), [occupiedHandleSignature])
  useEffect(() => scheduleNodeInternalsUpdate(id, updateNodeInternals), [id, portSignature, updateNodeInternals])
  if (data.editorKind !== 'action') return null

  const role = canvasNodeRole(manifest)
  const family = canvasNodeFamily(role)
  const metrics = canvasNodeMetrics(role)
  const localized = manifest ? localizeManifest(manifest, i18n.language) : undefined
  const label = manifest ? localizedNodeLabel(manifest, data.label, i18n.language) : data.label || data.nodeType
  const statusClass = selected ? 'studio-node-selected' : runtimeStatus ? `studio-node-${runtimeStatus}` : undefined
  return <div className={cn('studio-node relative shrink-0', `studio-node-${family}`, `studio-node-role-${role}`, `studio-node-zoom-${zoomTier}`)} data-role={role} data-testid={`studio-node-${id}`} style={{ width: metrics.width, height: metrics.height }}>
    {manifest?.inputPorts.map((port, index) => <PortHandle id={port.name} key={`in-${port.name}`} kind={port.kind} label={localized?.inputPortLabel(port.name) ?? port.name} placement={portPlacement('input', port.kind, port.name, index, manifest.inputPorts)} type="target" />)}
    <NodeSurface bindingSummaries={bindingSummaries} label={label} manifest={manifest} role={role} statusClass={statusClass} />
    <NodeLabel family={family} label={label} />
    {manifest?.outputPorts.map((port, index) => <PortHandle addLabel={t('studio.ports.addAfter', { label: localized?.outputPortLabel(port.name) ?? port.name })} id={port.name} key={`out-${port.name}`} kind={port.kind} label={localized?.outputPortLabel(port.name) ?? port.name} onHover={onSourceHover ? (active) => onSourceHover(id, port.name, active) : undefined} onQuickAdd={!data.disabled && onQuickAdd && (!occupiedHandles.has(port.name) || port.variadic) ? () => onQuickAdd(id, port.name, 'output') : undefined} placement={portPlacement('output', port.kind, port.name, index, manifest.outputPorts)} type="source" />)}
    {manifest?.bindingSlots.map((slot, index) => <PortHandle addLabel={t('studio.ports.addAfter', { label: localized?.bindingSlotLabel(slot.name) ?? slot.name })} id={`binding:${slot.name}`} key={slot.name} kind="binding" label={localized?.bindingSlotLabel(slot.name) ?? slot.name} onQuickAdd={!data.disabled && onQuickAdd && (!occupiedHandles.has(`binding:${slot.name}`) || slot.multiple) ? () => onQuickAdd(id, `binding:${slot.name}`, 'binding') : undefined} placement={bindingPlacement(index, manifest.bindingSlots.length)} resourceType={slot.resourceType} type="target" />)}
    {primary && <span className="absolute -left-2 -top-2 z-30 grid size-5 place-items-center rounded-full bg-primary text-primary-foreground" title={t('studio.primaryOutput')}><Star className="size-3 fill-current" /></span>}
    {data.disabled && <AlertTriangle className="absolute -right-2 -top-2 z-30 size-5 rounded-full bg-surface p-0.5 text-warning" />}
  </div>
})

const EMPTY_BINDINGS: NodeBindingSummary[] = []
let pendingInternals = new Set<string>()
let internalsUpdateScheduled = false

function scheduleNodeInternalsUpdate(id: string, update: (ids: string[]) => void) {
  pendingInternals.add(id)
  if (internalsUpdateScheduled) return
  internalsUpdateScheduled = true
  queueMicrotask(() => {
    const ids = [...pendingInternals]
    pendingInternals = new Set()
    internalsUpdateScheduled = false
    update(ids)
  })
}

function NodeSurface({ label, manifest, role, bindingSummaries, statusClass }: { label: string; manifest?: NodeManifest; role: CanvasNodeRole; bindingSummaries: NodeBindingSummary[]; statusClass?: string }) {
  return <div className={cn('studio-node-surface relative flex size-full items-center justify-center', statusClass)} title={bindingSummaries.map((binding) => `${binding.role}: ${binding.label}`).join('\n') || label}>
    {role === 'trigger' && <span className="studio-node-trigger-mark absolute -left-3 top-1/2 grid size-5 -translate-y-1/2 place-items-center rounded-full bg-warning text-background"><Zap className="size-3 fill-current" /></span>}
    <span className="studio-node-icon grid size-12 place-items-center rounded-md bg-muted text-foreground"><NodeIcon className="size-7" iconKey={manifest?.iconKey ?? roleIcon(role)} /></span>
  </div>
}

function NodeLabel({ label, family = 'compact' }: { label: string; family?: CanvasNodeFamily }) {
  return <div className={cn('studio-node-label pointer-events-none absolute left-1/2 z-10 w-40 -translate-x-1/2 text-center', family === 'agent' ? 'bottom-full mb-2' : 'top-full mt-2')}><strong className="block line-clamp-2 text-[12px] font-medium leading-4">{label}</strong></div>
}

type Placement = { position: Position; axis: number }

function PortHandle({ id, kind, label, placement, type, onQuickAdd, onHover, addLabel, resourceType }: { id: string; kind: PortKind | 'binding'; label: string; placement: Placement; type: 'source' | 'target'; onQuickAdd?: () => void; onHover?: (active: boolean) => void; addLabel?: string; resourceType?: ResourceType }) {
  const vertical = placement.position === Position.Left || placement.position === Position.Right
  const handleStyle = vertical ? { top: `${placement.axis}%` } : { left: `${placement.axis}%` }
  const labelClass = placement.position === Position.Left
    ? 'right-full mr-3 -translate-y-1/2'
    : placement.position === Position.Right
      ? 'left-full ml-3 -translate-y-1/2'
      : kind === 'error'
        ? 'top-full ml-2 -translate-y-1/2'
        : 'top-full mt-3 -translate-x-1/2'
  const labelStyle = vertical ? { top: `${placement.axis}%` } : { left: `${placement.axis}%` }
  const colorClass = kind === 'binding' ? `studio-port-${resourceType ?? 'binding'}` : kind === 'error' ? 'studio-port-error' : type === 'target' ? 'studio-port-input' : 'studio-port-output'
  return <>
    <Handle className={cn('studio-handle !absolute !z-30 !size-4 !border-0 !bg-transparent', colorClass)} id={id} onMouseEnter={() => onHover?.(true)} onMouseLeave={() => onHover?.(false)} position={placement.position} style={handleStyle} title={label} type={type}><span className={cn('studio-handle-mark block size-2.5 border-2 border-background', kind === 'binding' ? 'rotate-45 rounded-[2px]' : 'rounded-full')} /></Handle>
    <span className={cn('studio-port-label pointer-events-none absolute z-20 max-w-24 truncate text-[9px] font-medium text-muted-foreground', labelClass)} style={labelStyle}>{label}</span>
    {onQuickAdd && <button aria-label={addLabel} className={cn('studio-port-add nodrag absolute z-40 grid size-6 place-items-center rounded-full border border-border bg-surface text-muted-foreground hover:border-primary hover:text-primary', placement.position === Position.Bottom ? 'top-full mt-7 -translate-x-1/2' : 'left-full ml-7 -translate-y-1/2')} onClick={(event) => { event.stopPropagation(); onQuickAdd() }} style={labelStyle} title={addLabel} type="button"><Plus className="size-3.5" /></button>}
  </>
}

function bindingPlacement(index: number, count: number): Placement {
  return { position: Position.Bottom, axis: ((index + 1) / (count + 1)) * 100 }
}

function portPlacement(direction: 'input' | 'output', kind: PortKind, name: string, index: number, ports: { name: string; kind: PortKind }[]): Placement {
  if (direction === 'input') return { position: Position.Left, axis: ((index + 1) / (ports.length + 1)) * 100 }
  const orderedPorts = [...ports].sort((left, right) => Number(left.kind === 'error' || left.name === 'error') - Number(right.kind === 'error' || right.name === 'error'))
  const orderedIndex = orderedPorts.findIndex((port) => port.name === name && port.kind === kind)
  return { position: Position.Right, axis: ((orderedIndex + 1) / (ports.length + 1)) * 100 }
}

function roleIcon(role: CanvasNodeRole) {
  return ({ trigger: 'mouse-pointer-click', branch: 'split', merge: 'git-merge', loop: 'repeat-2', agent: 'bot', code: 'code-2', suspend: 'clock-3', approval: 'badge-check', sub_workflow: 'git-merge', error_handler: 'triangle-alert' } as Partial<Record<CanvasNodeRole, string>>)[role] ?? 'box'
}

export const AttachmentNode = memo(function AttachmentNode({ id, data, selected }: NodeProps<StudioNode>) {
  const { t } = useTranslation()
  const zoomTier = useCanvasRenderStore((state) => state.zoomTier)
  const onSourceHover = useCanvasRenderStore((state) => state.onSourceHover)
  if (data.editorKind !== 'binding') return null
  const metrics = canvasNodeMetrics('default', { kind: 'binding' })
  return <div className={cn('studio-node studio-node-attachment relative shrink-0', `studio-node-zoom-${zoomTier}`)} data-testid={`studio-node-${id}`} style={{ width: metrics.width, height: metrics.height }}>
    <div className={cn('studio-node-surface flex size-full items-center justify-center', selected && 'studio-node-selected')}><span className={cn('studio-node-icon grid size-12 place-items-center rounded-md bg-muted', `studio-resource-${data.resourceType}`)}><NodeIcon className="size-6" iconKey={attachmentIcon(data.resourceType)} /></span></div>
    <NodeLabel label={data.resourceName ?? data.label ?? t(`resourceTypes.${data.resourceType}`)} />
    <Handle className={cn('studio-handle studio-port-binding !absolute !top-[-8px] !size-4 !border-0 !bg-transparent')} id="resource" onMouseEnter={() => onSourceHover?.(id, 'resource', true)} onMouseLeave={() => onSourceHover?.(id, 'resource', false)} position={Position.Top} type="source"><span className="studio-handle-mark block size-2.5 rotate-45 rounded-[2px] border-2 border-background" /></Handle>
  </div>
})

function attachmentIcon(resourceType: string) { return ({ model: 'brain-circuit', mcp_tool: 'wrench', memory: 'memory-stick', rag: 'database', skill: 'sparkles' } as Record<string, string>)[resourceType] ?? 'box' }
