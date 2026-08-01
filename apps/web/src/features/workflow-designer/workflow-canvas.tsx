import {
  Background,
  BackgroundVariant,
  Controls,
  Handle,
  MiniMap,
  Position,
  ReactFlow,
  type Edge,
  type Node,
  type NodeProps,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'
import { Bot, CheckCircle2, MessageSquareText, Play, Plus, Save } from 'lucide-react'
import { useMemo } from 'react'

import { cn } from '../../shared/lib/cn'
import { Button } from '../../shared/ui/button'

type WorkflowNodeData = {
  label: string
  detail: string
  kind: 'trigger' | 'agent' | 'approval'
}

const nodeStyles = {
  trigger: 'bg-success/10 text-success',
  agent: 'bg-primary/10 text-primary',
  approval: 'bg-warning/10 text-warning',
}

const nodeIcons = {
  trigger: MessageSquareText,
  agent: Bot,
  approval: CheckCircle2,
}

function WorkflowNode({ data }: NodeProps<Node<WorkflowNodeData>>) {
  const Icon = nodeIcons[data.kind]
  return (
    <div className="min-w-52 overflow-hidden rounded-xl border border-border bg-surface shadow-lg">
      {data.kind !== 'trigger' && <Handle className="!size-2.5 !border-2 !border-background !bg-muted-foreground" position={Position.Left} type="target" />}
      <div className="flex items-center gap-3 border-b border-border px-3.5 py-3">
        <span className={cn('grid size-8 place-items-center rounded-lg', nodeStyles[data.kind])}><Icon className="size-4" /></span>
        <span><strong className="block text-xs">{data.label}</strong><span className="mt-0.5 block text-[10px] text-muted-foreground">{data.detail}</span></span>
      </div>
      <div className="flex items-center justify-between px-3.5 py-2.5 text-[10px] text-muted-foreground"><span>状态</span><span>配置完成</span></div>
      <Handle className="!size-2.5 !border-2 !border-background !bg-primary" position={Position.Right} type="source" />
    </div>
  )
}

const initialNodes: Array<Node<WorkflowNodeData>> = [
  { id: 'trigger', position: { x: 80, y: 220 }, data: { label: '收到用户消息', detail: 'Chat Trigger', kind: 'trigger' }, type: 'workflow' },
  { id: 'agent', position: { x: 390, y: 160 }, data: { label: '客服路由 Agent', detail: 'AI Agent · Tools', kind: 'agent' }, type: 'workflow' },
  { id: 'approval', position: { x: 720, y: 260 }, data: { label: '退款人工审批', detail: 'Approval', kind: 'approval' }, type: 'workflow' },
]

const initialEdges: Edge[] = [
  { id: 'trigger-agent', source: 'trigger', target: 'agent', animated: true },
  { id: 'agent-approval', source: 'agent', target: 'approval' },
]

export function WorkflowCanvas() {
  const nodeTypes = useMemo(() => ({ workflow: WorkflowNode }), [])

  return (
    <div className="flex h-full min-h-[calc(100vh-64px)] flex-col">
      <div className="flex h-15 shrink-0 items-center gap-3 border-b border-border bg-surface px-5">
        <div><h1 className="text-sm font-semibold">客户服务智能路由</h1><p className="mt-0.5 text-[10px] text-muted-foreground">Draft · v18 · 自动保存</p></div>
        <div className="flex-1" />
        <Button size="sm" variant="secondary"><Save className="size-3.5" />保存版本</Button>
        <Button size="sm"><Play className="size-3.5" />运行工作流</Button>
      </div>
      <div className="relative min-h-0 flex-1 bg-canvas">
        <ReactFlow
          defaultEdges={initialEdges}
          defaultNodes={initialNodes}
          fitView
          nodeTypes={nodeTypes}
          proOptions={{ hideAttribution: true }}
        >
          <Background color="var(--ui-canvas-dot)" gap={22} size={1} variant={BackgroundVariant.Dots} />
          <Controls className="!border-border !bg-surface !shadow-md" />
          <MiniMap className="!border !border-border !bg-surface" maskColor="color-mix(in srgb, var(--ui-background) 70%, transparent)" />
        </ReactFlow>
        <Button className="absolute left-5 top-5 z-10" size="sm"><Plus className="size-3.5" />添加节点</Button>
      </div>
    </div>
  )
}

