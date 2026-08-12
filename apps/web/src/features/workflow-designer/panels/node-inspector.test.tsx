import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { NodeManifest } from '../model/types'
import { NodeInspector } from './node-inspector'

const manifest: NodeManifest = {
  protocolVersion: '1.0', nodeType: 'set', version: 1, displayName: 'Set', description: 'Set fields', category: 'actions', keywords: ['set'], iconKey: 'box', executionStyle: 'action', capability: 'builtin', readiness: 'any', inputPorts: [], outputPorts: [], bindingSlots: [], parameterSchema: { type: 'object', properties: {} }, uiSchema: { canvas: { role: 'default' } }, providers: [], credentials: [], retryPolicy: { retryable: false, maxAttempts: 1, initialBackoffMs: 0, maxBackoffMs: 0 }, sandboxRequired: false, supportsMock: true, sideEffectLevel: 'none',
}

const agentManifest: NodeManifest = {
  ...manifest,
  nodeType: 'agent',
  parameterSchema: { type: 'object', properties: { systemPrompt: { type: 'string' }, userQuestion: { type: 'string', templatable: true }, maxIterations: { type: 'integer', default: 12 } } },
  uiSchema: { canvas: { role: 'agent' }, fields: { systemPrompt: { control: 'prompt' }, userQuestion: { control: 'text' }, maxIterations: { control: 'number' } } },
}

const modelManifest: NodeManifest = {
  ...manifest,
  nodeType: 'model',
  outputPorts: [{ name: 'main', kind: 'main', required: false, variadic: false }],
  parameterSchema: { type: 'object', properties: { prompt: { type: 'string' }, userQuestion: { type: 'string', templatable: true } } },
  uiSchema: { canvas: { role: 'default' }, fields: { prompt: { control: 'prompt' }, userQuestion: { control: 'text' } } },
  outputSchema: { type: 'object', properties: { text: { type: 'string' } } },
  outputProjectionSchema: {},
  contextWriteCapability: true,
}

describe('NodeInspector details view', () => {
  it('provides the fixed Parameters, Input, Output and Trace views', () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><NodeInspector data={{ editorKind: 'action', nodeType: 'set', typeVersion: 1, label: 'Set', key: 'set', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }} manifest={manifest} nodeId="node-1" onChange={vi.fn()} onDelete={vi.fn()} onRun={vi.fn()} resources={{}} /></QueryClientProvider>)

    expect(screen.getByTestId('node-details-view')).toHaveClass('w-[480px]')
    const tabs = [/Parameters|参数/, /Input|输入/, /Output|输出/, /Trace/]
    for (const name of tabs) expect(screen.getByRole('tab', { name })).toBeInTheDocument()
    expect(screen.getByRole('tabpanel', { name: /Parameters|参数/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /More node actions|更多节点操作/ })).toHaveAttribute('aria-haspopup', 'menu')
  })

  it('presents Agent prompt and one user question before an inline advanced section', () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><NodeInspector data={{ editorKind: 'action', nodeType: 'agent', typeVersion: 1, label: 'Agent', key: 'agent', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }} manifest={agentManifest} nodeId="agent-1" onChange={vi.fn()} onDelete={vi.fn()} resources={{}} /></QueryClientProvider>)

    expect(screen.getByTestId('parameter-systemPrompt')).toBeInTheDocument()
    expect(screen.getByTestId('parameter-userQuestion').querySelectorAll('input')).toHaveLength(1)
    expect(screen.getByText('Advanced configuration')).toBeInTheDocument()
    expect(screen.getByTestId('parameter-maxIterations')).toBeInTheDocument()
    expect(screen.getByText('calls')).toBeInTheDocument()
  })

  it('moves node disablement into the more menu and shows disabled status in the header', () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const onChange = vi.fn()
    render(<QueryClientProvider client={client}><NodeInspector data={{ editorKind: 'action', nodeType: 'set', typeVersion: 1, label: 'Set', key: 'set', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: true }} manifest={manifest} nodeId="node-1" onChange={onChange} onDelete={vi.fn()} resources={{}} /></QueryClientProvider>)

    expect(screen.getByText(/Disabled|禁用/)).toBeInTheDocument()
    expect(screen.queryByRole('checkbox', { name: /Disabled|禁用/ })).not.toBeInTheDocument()
    fireEvent.pointerDown(screen.getByRole('button', { name: /More node actions|更多节点操作/ }), { button: 0, ctrlKey: false })
    fireEvent.click(screen.getByRole('menuitem', { name: /Enable node|启用节点/ }))
    expect(onChange).toHaveBeenCalledWith({ disabled: false })
  })

  it('shows Model prompt and one user question without the raw parameters field', () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><NodeInspector data={{ editorKind: 'action', nodeType: 'model', typeVersion: 1, label: 'Model', key: 'model', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }} manifest={modelManifest} nodeId="model-1" onChange={vi.fn()} onDelete={vi.fn()} resources={{}} /></QueryClientProvider>)

    const prompt = screen.getByTestId('parameter-prompt')
    const question = screen.getByTestId('parameter-userQuestion')
    expect(prompt.compareDocumentPosition(question) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(question.querySelectorAll('input')).toHaveLength(1)
    expect(screen.queryByTestId('parameter-parameters')).not.toBeInTheDocument()
  })

  it('configures custom outputs and context writes through dialogs', () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const onChange = vi.fn()
    const referenceCatalog = { inputs: [], outputs: [], contexts: [{ id: 'contexts.session', label: 'session', path: 'contexts.session', type: 'object', children: [{ id: 'contexts.session.answer', label: 'answer', path: 'contexts.session.answer', expression: '${{ contexts.session.answer }}', type: 'string', children: [] }] }] }
    render(<QueryClientProvider client={client}><NodeInspector data={{ editorKind: 'action', nodeType: 'model', typeVersion: 1, label: 'Model', key: 'model', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }} manifest={modelManifest} nodeId="model-1" onChange={onChange} onDelete={vi.fn()} referenceCatalog={referenceCatalog} resources={{}} /></QueryClientProvider>)

    fireEvent.click(screen.getByRole('button', { name: /Add custom output|添加自定义输出/ }))
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    fireEvent.change(screen.getByLabelText(/Output name|输出名称/), { target: { value: 'answer' } })
    fireEvent.change(screen.getByLabelText(/Expression|表达式/), { target: { value: '${{ item.json.text }}' } })
    fireEvent.click(screen.getByRole('button', { name: /Save|保存/ }))
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ outputProjection: expect.objectContaining({ main: expect.objectContaining({ answer: expect.objectContaining({ expression: '${{ item.json.text }}' }) }) }) }))

    fireEvent.click(screen.getByRole('button', { name: /Add global variable write|添加全局变量写入/ }))
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('combobox', { name: /Global variable|全局变量/ }))
    fireEvent.click(screen.getByRole('option', { name: 'session.answer' }))
    fireEvent.click(screen.getByRole('button', { name: /Save|保存/ }))
    expect(onChange).toHaveBeenCalledWith({ contextWrites: [{ operation: 'set', path: 'session.answer', value: '' }] })
  })
})
