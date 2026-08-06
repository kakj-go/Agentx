import { Handle, Position, type NodeProps } from '@xyflow/react'
import { AlertTriangle } from 'lucide-react'

import { cn } from '../../../shared/lib/cn'
import type { NodeManifest, StudioNode } from '../model/types'
import { NodeIcon } from './node-icon'
import { nodeShape } from './node-appearance'

export function ManifestNode({ id, data, selected, manifest, runtimeStatus }: NodeProps<StudioNode> & { manifest?: NodeManifest; runtimeStatus?: string }) {
  if (data.editorKind !== 'action') return null
  const shape = nodeShape(manifest)
  const statusClass = selected ? 'border-primary ring-2 ring-primary/20' : runtimeStatus === 'running' ? 'border-warning ring-2 ring-warning/20' : runtimeStatus === 'succeeded' ? 'border-success' : runtimeStatus === 'failed' ? 'border-danger' : 'border-border'
  return <div className={cn('studio-node relative w-[228px] border bg-surface shadow-sm transition-shadow', `studio-node-${shape}`, statusClass)} data-testid={`studio-node-${id}`}>
    {manifest?.inputPorts.map((port, index) => <PortHandle key={`in-${port.name}`} id={port.name} kind={port.kind} label={port.name} position={Position.Left} style={{ top: 45 + index * 20 }} type="target" />)}
    <div className="flex min-h-[76px] items-center gap-3 px-3 py-3"><span className={cn('grid size-9 shrink-0 place-items-center text-primary', shape === 'trigger' ? 'rounded-full bg-primary/12' : shape === 'branch' ? 'rounded-md bg-warning/15 text-warning' : shape === 'code' ? 'rounded-sm bg-danger/10 text-danger' : shape === 'agent' ? 'rounded-xl bg-primary/15' : 'rounded-lg bg-primary/10')}><NodeIcon className="size-4" iconKey={manifest?.iconKey ?? 'box'} /></span><span className="min-w-0 flex-1"><strong className="block truncate text-xs font-semibold">{data.label}</strong><span className="mt-1 block truncate text-[10px] text-muted-foreground">{manifest?.displayName ?? data.nodeType}</span></span>{data.disabled && <AlertTriangle className="size-3.5 shrink-0 text-warning" />}</div>
    <div className="flex items-center justify-between border-t border-border px-3 py-1.5 text-[9px] uppercase tracking-wide text-muted-foreground"><span>{manifest?.category ?? 'node'}</span><span>v{data.typeVersion}</span></div>
    {manifest?.outputPorts.map((port, index) => <PortHandle key={`out-${port.name}`} id={port.name} kind={port.kind} label={port.name} position={Position.Right} style={{ top: 45 + index * 20 }} type="source" />)}
    {manifest?.bindingSlots.map((slot, index) => <div className="absolute bottom-[-22px]" key={slot.name} style={{ left: 25 + index * Math.min(54, 180 / Math.max(manifest.bindingSlots.length, 1)) }}><PortHandle id={`binding:${slot.name}`} kind="binding" label={slot.name.replace(/^ai_/, '')} position={Position.Top} type="target" /></div>)}
  </div>
}

function PortHandle({ id, kind, label, position, style, type }: { id: string; kind: 'main' | 'error' | 'binding'; label: string; position: Position; style?: React.CSSProperties; type: 'source' | 'target' }) {
  const isBinding = kind === 'binding'
  return <span className={cn('absolute z-10 flex items-center gap-1 text-[8px] font-medium uppercase tracking-wide text-muted-foreground', position === Position.Left ? 'left-[-51px] flex-row-reverse' : position === Position.Right ? 'right-[-51px]' : 'top-[-19px] left-1/2 -translate-x-1/2 flex-col-reverse')} style={style}><Handle className={cn('!relative !left-auto !top-auto !m-0 !size-3 !border-2 !border-background shadow-sm', kind === 'error' ? '!bg-danger' : isBinding ? '!bg-warning' : position === Position.Left ? '!bg-muted-foreground' : '!bg-primary')} id={id} position={position} type={type} /><span className="max-w-12 truncate">{label}</span></span>
}

export function AttachmentNode({ id, data, selected }: NodeProps<StudioNode>) {
  if (data.editorKind !== 'binding') return null
  return <div className={cn('studio-node-attachment flex h-[62px] w-[184px] items-center gap-2 border border-dashed bg-surface px-3 shadow-sm', selected ? 'border-warning ring-2 ring-warning/15' : 'border-border')} data-testid={`studio-node-${id}`}>
    <span className="grid size-8 shrink-0 place-items-center rounded-md bg-warning/10 text-warning"><NodeIcon className="size-4" iconKey={attachmentIcon(data.resourceType)} /></span><span className="min-w-0"><strong className="block truncate text-[11px] font-semibold">{data.resourceName ?? data.label}</strong><span className="block truncate text-[9px] text-muted-foreground">{data.resourceType}</span></span>
    <Handle className="!size-3 !border-2 !border-background !bg-warning" id="resource" position={Position.Right} type="source" />
  </div>
}

function attachmentIcon(resourceType: string) {
  return ({ model: 'brain-circuit', mcp_tool: 'wrench', memory: 'memory-stick', rag: 'database', skill: 'sparkles' } as Record<string, string>)[resourceType] ?? 'box'
}
