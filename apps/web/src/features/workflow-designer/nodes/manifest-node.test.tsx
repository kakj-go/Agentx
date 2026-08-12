import { ReactFlowProvider, type NodeProps } from '@xyflow/react'
import { act, fireEvent, render, screen } from '@testing-library/react'
import { Profiler } from 'react'
import { describe, expect, it, vi } from 'vitest'

import type { CanvasNodeRole, NodeManifest, StudioNode } from '../model/types'
import { useCanvasRenderStore } from '../store/canvas-render-store'
import { ManifestNode } from './manifest-node'

const roles: CanvasNodeRole[] = ['default', 'trigger', 'branch', 'flow', 'merge', 'loop', 'suspend', 'approval', 'sub_workflow', 'agent', 'code', 'error_handler']

const manifest = (role: CanvasNodeRole): NodeManifest => ({
  protocolVersion: '1.0', nodeType: role, version: 1, displayName: role, description: `${role} node`, category: 'actions', keywords: [role], iconKey: 'box', executionStyle: 'action', capability: 'builtin', readiness: 'any',
  inputPorts: [{ name: 'main', kind: 'main', required: false, variadic: false }], outputPorts: [{ name: 'error', kind: 'error', required: false, variadic: false }], bindingSlots: role === 'agent' ? [{ name: 'ai_model', resourceType: 'model', required: false, multiple: false }] : [],
  parameterSchema: {}, uiSchema: { canvas: { role } }, providers: [], credentials: [], retryPolicy: { retryable: false, maxAttempts: 1, initialBackoffMs: 0, maxBackoffMs: 0 }, sandboxRequired: false, supportsMock: true, sideEffectLevel: 'none',
})

const props = (role: CanvasNodeRole): NodeProps<StudioNode> => ({
  id: `node-${role}`, type: 'manifest', data: { editorKind: 'action', nodeType: role, typeVersion: 1, label: `${role} label`, parameters: {}, resourceReferences: [], settings: {}, disabled: false }, selected: false,
} as unknown as NodeProps<StudioNode>)

describe('ManifestNode roles', () => {
  for (const role of roles) it(`renders the controlled ${role} role`, () => {
    const value = manifest(role)
    useCanvasRenderStore.setState({ manifests: new Map([[`${role}@1`, value]]), runtimeStatuses: new Map(), bindingSummaries: new Map(), occupiedHandlesByNodeId: new Map(), zoomTier: 'full' })
    render(<ReactFlowProvider><ManifestNode {...props(role)} /></ReactFlowProvider>)
    expect(screen.getByTestId(`studio-node-node-${role}`)).toHaveAttribute('data-role', role)
  })

  it('keeps handles interactive at low zoom and shows actual Agent bindings', () => {
    const onSourceHover = vi.fn()
    useCanvasRenderStore.setState({
      manifests: new Map([['agent@1', manifest('agent')]]), runtimeStatuses: new Map(),
      bindingSummaries: new Map([['node-agent', [{ role: 'ai_model', resourceType: 'model', label: 'Production model' }]]]),
      occupiedHandlesByNodeId: new Map(),
      zoomTier: 'compact', onQuickAdd: () => undefined, onSourceHover,
    })
    render(<ReactFlowProvider><ManifestNode {...props('agent')} /></ReactFlowProvider>)
    const node = screen.getByTestId('studio-node-node-agent')
    expect(node).toHaveClass('studio-node-zoom-compact')
    expect(screen.getByTitle(/Production model/)).toBeInTheDocument()
    expect(node.querySelectorAll('.react-flow__handle')).toHaveLength(3)
    expect(screen.getByRole('button', { name: /Add node after error|在错误后添加节点/i })).toBeInTheDocument()
    const source = node.querySelector('.react-flow__handle.source')!
    fireEvent.mouseEnter(source)
    fireEvent.mouseLeave(source)
    expect(onSourceHover).toHaveBeenNthCalledWith(1, 'node-agent', 'error', true)
    expect(onSourceHover).toHaveBeenNthCalledWith(2, 'node-agent', 'error', false)
  })

  it('places error outputs on the right below normal outputs', () => {
    const value = manifest('default')
    value.outputPorts = [
      { name: 'error', kind: 'error', required: false, variadic: false },
      { name: 'main', kind: 'main', required: false, variadic: false },
    ]
    useCanvasRenderStore.setState({ manifests: new Map([['default@1', value]]), runtimeStatuses: new Map(), bindingSummaries: new Map(), occupiedHandlesByNodeId: new Map(), zoomTier: 'full' })
    render(<ReactFlowProvider><ManifestNode {...props('default')} /></ReactFlowProvider>)
    const node = screen.getByTestId('studio-node-node-default')
    const main = node.querySelector<HTMLElement>('.react-flow__handle.source[data-handleid="main"]')!
    const error = node.querySelector<HTMLElement>('.react-flow__handle.source[data-handleid="error"]')!
    expect(main).toHaveClass('react-flow__handle-right')
    expect(error).toHaveClass('react-flow__handle-right')
    expect(Number.parseFloat(error.style.top)).toBeGreaterThan(Number.parseFloat(main.style.top))
    const mainLabel = node.querySelector<HTMLElement>('[data-port-type="source"][data-port-id="main"]')!
    const errorLabel = node.querySelector<HTMLElement>('[data-port-type="source"][data-port-id="error"]')!
    expect(mainLabel.style.top).toBe(main.style.top)
    expect(errorLabel.style.top).toBe(error.style.top)
    expect(main).toHaveClass('!grid', '!place-items-center')
    expect(mainLabel).toHaveClass('items-center', 'leading-none')
  })

  it('centers bottom resource labels beneath their handles', () => {
    const value = manifest('agent')
    useCanvasRenderStore.setState({ manifests: new Map([['agent@1', value]]), runtimeStatuses: new Map(), bindingSummaries: new Map(), occupiedHandlesByNodeId: new Map(), zoomTier: 'full' })
    render(<ReactFlowProvider><ManifestNode {...props('agent')} /></ReactFlowProvider>)
    const node = screen.getByTestId('studio-node-node-agent')
    const handle = node.querySelector<HTMLElement>('.react-flow__handle.target[data-handleid="binding:ai_model"]')!
    const label = node.querySelector<HTMLElement>('[data-port-type="target"][data-port-id="binding:ai_model"]')!
    expect(label.style.left).toBe(handle.style.left)
    expect(label).toHaveClass('w-16', 'text-center', '-translate-x-1/2')
  })

  it('keeps quick add visible only for unoccupied or repeatable ports', () => {
    const single = manifest('agent')
    useCanvasRenderStore.setState({
      manifests: new Map([['agent@1', single]]), runtimeStatuses: new Map(), bindingSummaries: new Map(),
      occupiedHandlesByNodeId: new Map([['node-agent', 'binding:ai_model\u0001error']]), zoomTier: 'full', onQuickAdd: () => undefined,
    })
    render(<ReactFlowProvider><ManifestNode {...props('agent')} /></ReactFlowProvider>)
    const node = screen.getByTestId('studio-node-node-agent')
    expect(node.querySelectorAll('.studio-port-add')).toHaveLength(0)

    const repeatable = manifest('agent')
    repeatable.outputPorts[0].variadic = true
    repeatable.bindingSlots[0].multiple = true
    act(() => useCanvasRenderStore.setState({ manifests: new Map([['agent@1', repeatable]]) }))
    expect(node.querySelectorAll('.studio-port-add')).toHaveLength(2)
  })

  it('rerenders only the node whose runtime status changes', () => {
    const value = manifest('default')
    useCanvasRenderStore.setState({ manifests: new Map([['default@1', value]]), runtimeStatuses: new Map(), bindingSummaries: new Map(), occupiedHandlesByNodeId: new Map(), zoomTier: 'full' })
    const first = vi.fn()
    const second = vi.fn()
    render(<ReactFlowProvider><Profiler id="first" onRender={first}><ManifestNode {...props('default')} id="first" /></Profiler><Profiler id="second" onRender={second}><ManifestNode {...props('default')} id="second" /></Profiler></ReactFlowProvider>)
    first.mockClear()
    second.mockClear()

    act(() => useCanvasRenderStore.setState({ runtimeStatuses: new Map([['first', 'running']]) }))
    expect(first).toHaveBeenCalledOnce()
    expect(second).not.toHaveBeenCalled()
  })
})
