import { Handle, Position, type NodeProps } from '@xyflow/react'
import { AlertTriangle } from 'lucide-react'

import { cn } from '../../../shared/lib/cn'
import type { NodeManifest, StudioNode } from '../model/types'
import { NodeIcon } from './node-icon'

export function ManifestNode({ id, data, selected, manifest, runtimeStatus }: NodeProps<StudioNode> & { manifest?: NodeManifest; runtimeStatus?: string }) {
  if (data.editorKind !== 'action') return null
  return <div className={cn('w-[220px] border bg-surface shadow-md', selected ? 'border-primary ring-2 ring-primary/15' : runtimeStatus === 'running' ? 'border-warning ring-2 ring-warning/15' : runtimeStatus === 'succeeded' ? 'border-success' : runtimeStatus === 'failed' ? 'border-danger' : 'border-border')} data-testid={`studio-node-${id}`}>
    {manifest?.inputPorts.map((port, index) => <Handle className={cn('!size-2.5 !border-2 !border-background', port.kind === 'error' ? '!bg-danger' : '!bg-muted-foreground')} id={port.name} key={port.name} position={Position.Left} style={{ top: 42 + index * 18 }} type="target" />)}
    <div className="flex min-h-[72px] items-center gap-3 px-3 py-2.5"><span className="grid size-9 shrink-0 place-items-center rounded-md bg-primary/10 text-primary"><NodeIcon className="size-4" iconKey={manifest?.iconKey ?? 'box'} /></span><span className="min-w-0 flex-1"><strong className="block truncate text-xs font-semibold">{data.label}</strong><span className="mt-1 block truncate text-[10px] text-muted-foreground">{manifest?.displayName ?? data.nodeType}</span></span>{data.disabled && <AlertTriangle className="size-3.5 text-warning" />}</div>
    <div className="border-t border-border px-3 py-1.5 text-[9px] uppercase text-muted-foreground">{manifest?.category ?? 'node'} · v{data.typeVersion}</div>
    {manifest?.outputPorts.map((port, index) => <Handle className={cn('!size-2.5 !border-2 !border-background', port.kind === 'error' ? '!bg-danger' : '!bg-primary')} id={port.name} key={port.name} position={Position.Right} style={{ top: 42 + index * 18 }} type="source" />)}
    {manifest?.bindingSlots.map((slot, index) => <Handle className="!size-3 !border-2 !border-background !bg-warning" id={`binding:${slot.name}`} key={slot.name} position={Position.Bottom} style={{ left: 32 + index * 32 }} type="target" />)}
  </div>
}

export function AttachmentNode({ id, data, selected }: NodeProps<StudioNode>) {
  if (data.editorKind !== 'binding') return null
  return <div className={cn('flex h-[58px] w-[180px] items-center gap-2 border border-dashed bg-surface px-3 shadow-sm', selected ? 'border-warning ring-2 ring-warning/15' : 'border-border')} data-testid={`studio-node-${id}`}>
    <span className="grid size-8 shrink-0 place-items-center rounded-md bg-warning/10 text-warning"><NodeIcon className="size-4" iconKey={attachmentIcon(data.resourceType)} /></span><span className="min-w-0"><strong className="block truncate text-[11px] font-semibold">{data.resourceName ?? data.label}</strong><span className="block truncate text-[9px] text-muted-foreground">{data.resourceType}</span></span>
    <Handle className="!size-3 !border-2 !border-background !bg-warning" id="resource" position={Position.Right} type="source" />
  </div>
}

function attachmentIcon(resourceType: string) {
  return ({ model: 'brain-circuit', mcp_tool: 'wrench', memory: 'memory-stick', rag: 'database', skill: 'sparkles' } as Record<string, string>)[resourceType] ?? 'box'
}
