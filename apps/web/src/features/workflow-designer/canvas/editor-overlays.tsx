import { Handle, NodeResizer, Position, useUpdateNodeInternals, type NodeProps } from '@xyflow/react'
import { ChevronDown, ChevronRight, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { cn } from '../../../shared/lib/cn'
import type { CanvasNode } from '../model/types'

const NOTE_COLORS = ['yellow', 'blue', 'green', 'rose'] as const

export function AnnotationNode({ id, data, selected }: NodeProps<CanvasNode>) {
  const { t } = useTranslation()
  const note = data.editorKind === 'annotation' ? data : undefined
  const [editing, setEditing] = useState(false)
  const [text, setText] = useState(note?.text ?? '')
  const updateNodeInternals = useUpdateNodeInternals()
  useEffect(() => setText(note?.text ?? ''), [note?.text])
  useEffect(() => updateNodeInternals(id), [id, note?.text, updateNodeInternals])
  if (!note) return null

  const commitText = () => {
    setEditing(false)
    if (text !== note.text) note.onChange({ text })
  }
  return <article className={cn('studio-note relative h-full w-full overflow-hidden border shadow-sm', `studio-note-${note.color ?? 'yellow'}`, selected && 'ring-2 ring-primary/40')} data-testid={`studio-note-${note.annotationId}`} onDoubleClick={(event) => { event.stopPropagation(); setEditing(true) }}>
    <NodeResizer color="var(--ui-primary)" isVisible={selected} minHeight={80} minWidth={150} onResize={(_, frame) => note.onResize(frame)} onResizeEnd={() => { note.onResizeEnd(); updateNodeInternals(id) }} onResizeStart={note.onResizeStart} />
    <header className="drag-handle flex h-8 items-center gap-1 border-b border-black/10 px-2">
      <span className="flex-1 text-[9px] font-semibold uppercase text-black/55">{t('studio.note.title')}</span>
      <div className="nodrag flex items-center gap-1">{NOTE_COLORS.map((color) => <button aria-label={t('studio.note.color', { color })} className={cn('size-3 rounded-full border border-black/15', `studio-note-swatch-${color}`, note.color === color && 'ring-1 ring-black/40')} key={color} onClick={() => note.onChange({ color })} type="button" />)}<button aria-label={t('studio.note.delete')} className="ml-1 grid size-5 place-items-center rounded text-black/50 hover:bg-black/10 hover:text-black" onClick={note.onRemove} type="button"><Trash2 className="size-3" /></button></div>
    </header>
    {editing ? <textarea autoFocus className="nodrag nowheel h-[calc(100%-2rem)] w-full resize-none bg-transparent p-3 text-xs leading-5 text-black/75 outline-none" onBlur={commitText} onChange={(event) => setText(event.target.value)} onKeyDown={(event) => { if (event.key === 'Escape') { setText(note.text); setEditing(false) }; if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') commitText() }} value={text} /> : <div className="h-[calc(100%-2rem)] whitespace-pre-wrap break-words p-3 text-xs leading-5 text-black/75">{note.text}</div>}
  </article>
}

export function GroupNode({ data }: NodeProps<CanvasNode>) {
  const { t } = useTranslation()
  const group = data.editorKind === 'group' ? data : undefined
  if (!group) return null
  if (group.collapsed) return <div className="studio-group-proxy relative flex h-16 w-60 items-center gap-3 border border-primary/50 bg-surface px-3 shadow-sm" data-testid={`studio-group-${group.groupId}`}>
    <Handle className="!size-3 !border-2 !border-background !bg-muted-foreground" id="group-in" isConnectable={false} position={Position.Left} type="target" />
    <button aria-label={t('studio.group.expand')} className="nodrag grid size-8 shrink-0 place-items-center rounded-md bg-primary/10 text-primary hover:bg-primary/15" onClick={group.onToggle} type="button"><ChevronRight className="size-4" /></button>
    <div className="min-w-0 flex-1"><strong className="block truncate text-xs">{group.label}</strong><span className="text-[9px] text-muted-foreground">{t('studio.group.members', { count: group.memberCount })}</span></div>
    <button aria-label={t('studio.group.delete')} className="nodrag grid size-7 place-items-center rounded-md text-muted-foreground hover:bg-muted hover:text-danger" onClick={group.onRemove} type="button"><Trash2 className="size-3.5" /></button>
    <Handle className="!size-3 !border-2 !border-background !bg-primary" id="group-out" isConnectable={false} position={Position.Right} type="source" />
  </div>
  return <section className="studio-group-expanded relative h-full w-full border border-dashed border-primary/45 bg-primary/[0.025]" data-testid={`studio-group-${group.groupId}`}>
    <header className="drag-handle pointer-events-auto absolute left-0 top-0 flex h-8 max-w-full items-center gap-2 rounded-br-md border-b border-r border-primary/25 bg-surface/95 px-2 shadow-sm">
      <button aria-label={t('studio.group.collapse')} className="nodrag grid size-5 place-items-center rounded text-primary hover:bg-primary/10" onClick={group.onToggle} type="button"><ChevronDown className="size-3.5" /></button>
      <strong className="max-w-48 truncate text-[10px]">{group.label}</strong><span className="text-[9px] text-muted-foreground">{group.memberCount}</span>
      <button aria-label={t('studio.group.delete')} className="nodrag grid size-5 place-items-center rounded text-muted-foreground hover:bg-muted hover:text-danger" onClick={group.onRemove} type="button"><Trash2 className="size-3" /></button>
    </header>
  </section>
}
