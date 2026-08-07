import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { NodeManifest } from '../model/types'
import { NodeInspector } from './node-inspector'

const manifest: NodeManifest = {
  protocolVersion: '1.0', nodeType: 'set', version: 1, displayName: 'Set', description: 'Set fields', category: 'actions', keywords: ['set'], iconKey: 'box', executionStyle: 'action', capability: 'builtin', readiness: 'any', inputPorts: [], outputPorts: [], bindingSlots: [], parameterSchema: { type: 'object', properties: {} }, uiSchema: { canvas: { role: 'default' } }, providers: [], credentials: [], retryPolicy: { retryable: false, maxAttempts: 1, initialBackoffMs: 0, maxBackoffMs: 0 }, sandboxRequired: false, supportsMock: true, sideEffectLevel: 'none',
}

describe('NodeInspector details view', () => {
  it('provides the fixed Parameters, Input, Output and Trace views', () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><NodeInspector data={{ editorKind: 'action', nodeType: 'set', typeVersion: 1, label: 'Set', parameters: {}, resourceReferences: [], settings: {}, disabled: false }} manifest={manifest} nodeId="node-1" onChange={vi.fn()} onDelete={vi.fn()} onRun={vi.fn()} resources={{}} /></QueryClientProvider>)

    expect(screen.getByTestId('node-details-view')).toHaveClass('w-[480px]')
    const tabs = [/Parameters|参数/, /Input|输入/, /Output|输出/, /Trace/]
    for (const name of tabs) expect(screen.getByRole('tab', { name })).toBeInTheDocument()
    expect(screen.getByRole('tabpanel', { name: /Parameters|参数/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /More node actions|更多节点操作/ })).toHaveAttribute('aria-haspopup', 'menu')
  })
})
