import { ReactFlowProvider, type NodeProps } from '@xyflow/react'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import type { CanvasNodeRole, NodeManifest, StudioNode } from '../model/types'
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
    render(<ReactFlowProvider><ManifestNode {...props(role)} manifest={manifest(role)} /></ReactFlowProvider>)
    expect(screen.getByTestId(`studio-node-node-${role}`)).toHaveAttribute('data-role', role)
  })

  it('keeps handles interactive at low zoom and shows actual Agent bindings', () => {
    render(<ReactFlowProvider><ManifestNode {...props('agent')} bindingSummaries={[{ role: 'ai_model', resourceType: 'model', label: 'Production model' }]} manifest={manifest('agent')} onQuickAdd={() => undefined} zoom={0.5} /></ReactFlowProvider>)
    const node = screen.getByTestId('studio-node-node-agent')
    expect(node).toHaveClass('studio-node-low-zoom')
    expect(screen.getByText(/Production model/)).toBeInTheDocument()
    expect(node.querySelectorAll('.react-flow__handle')).toHaveLength(3)
    expect(screen.getByRole('button', { name: /Add node after error|在错误后添加节点/i })).toBeInTheDocument()
  })
})
