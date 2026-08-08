import { ReactFlowProvider, type NodeProps } from '@xyflow/react'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { CanvasNode } from '../model/types'
import { AnnotationNode, GroupNode } from './editor-overlays'

describe('editor overlay nodes', () => {
  it('edits and colors a Sticky Note without entering the Workflow Definition', () => {
    const onChange = vi.fn()
    const props = { id: 'annotation:note', selected: true, data: { editorKind: 'annotation', annotationId: 'note', text: 'Initial note', color: 'yellow', onChange, onRemove: vi.fn(), onResizeStart: vi.fn(), onResize: vi.fn(), onResizeEnd: vi.fn() } } as unknown as NodeProps<CanvasNode>
    render(<ReactFlowProvider><AnnotationNode {...props} /></ReactFlowProvider>)

    fireEvent.doubleClick(screen.getByTestId('studio-note-note'))
    const editor = screen.getByRole('textbox')
    fireEvent.change(editor, { target: { value: 'Updated note' } })
    fireEvent.blur(editor)
    expect(onChange).toHaveBeenCalledWith({ text: 'Updated note' })
    fireEvent.click(screen.getByRole('button', { name: /Note color: blue|便签颜色：blue/ }))
    expect(onChange).toHaveBeenCalledWith({ color: 'blue' })
  })

  it('renders a collapsed Group proxy with aggregate handles and ungroup action', () => {
    const onToggle = vi.fn()
    const onRemove = vi.fn()
    const props = { id: 'group:main', selected: false, data: { editorKind: 'group', groupId: 'main', label: 'Main', collapsed: true, memberCount: 3, onToggle, onRemove } } as unknown as NodeProps<CanvasNode>
    render(<ReactFlowProvider><GroupNode {...props} /></ReactFlowProvider>)

    const group = screen.getByTestId('studio-group-main')
    expect(group.querySelector('[data-nodeid]')).not.toBeInTheDocument()
    expect(group.querySelectorAll('.react-flow__handle')).toHaveLength(2)
    fireEvent.click(screen.getByRole('button', { name: /Expand group|展开分组/ }))
    fireEvent.click(screen.getByRole('button', { name: /Ungroup|解除分组/ }))
    expect(onToggle).toHaveBeenCalledOnce()
    expect(onRemove).toHaveBeenCalledOnce()
  })
})
