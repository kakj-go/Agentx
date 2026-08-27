import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ToastProvider } from '../../../shared/ui/toast'
import type { NodeManifest, ReferenceCatalog } from '../model/types'
import { NodeInspector } from './node-inspector'

const manifest: NodeManifest = {
  protocolVersion: '2.0', nodeType: 'set', version: 1, displayName: 'Set', description: 'Set fields', category: 'actions', keywords: ['set'], iconKey: 'box', executionStyle: 'action', capability: 'builtin', readiness: 'any', inputPorts: [], outputPorts: [], bindingSlots: [], parameterSchema: { type: 'object', properties: {} }, uiSchema: { canvas: { role: 'default' } }, providers: [], credentials: [], retryPolicy: { retryable: false, maxAttempts: 1, initialBackoffMs: 0, maxBackoffMs: 0 }, sandboxRequired: false, supportsMock: true, sideEffectLevel: 'none',
}

const agentManifest: NodeManifest = {
  ...manifest,
  nodeType: 'agent',
  version: 2,
  bindingSlots: [
    { name: 'model', resourceType: 'model', placement: 'inspector', required: true, multiple: false },
    { name: 'workspace_sandbox', resourceType: 'sandbox_profile', placement: 'inspector', required: false, multiple: false },
    { name: 'mcp_tools', resourceType: 'mcp_tool', placement: 'canvas', required: false, multiple: true },
  ],
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

const filterManifest: NodeManifest = {
  ...manifest,
  nodeType: 'filter',
  parameterSchema: { type: 'object', properties: { condition: { "x-agentx-dynamicValue": { modes: ['literal', 'reference', 'expression'], allowedNamespaces: ['inputs', 'outputs', 'contexts', 'item'], acceptedCardinality: ['single'], missingPolicies: ['error'], recursive: false } } } },
  uiSchema: { canvas: { role: 'default' }, fields: { condition: { control: 'expression' } } },
}

const executionCatalog: ReferenceCatalog = {
  inputs: [],
  outputs: [],
  contexts: [{ id: 'contexts.session', label: 'session', path: 'contexts.session', type: 'object', children: [] }],
  execution: [{ id: 'execution.root', label: 'Execution information', path: 'execution', children: [] }],
}

describe('NodeInspector details view', () => {
  it('provides the fixed Parameters, Input, Output and Trace views', () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><ToastProvider><NodeInspector data={{ editorKind: 'action', nodeType: 'set', typeVersion: 1, label: 'Set', key: 'set', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }} manifest={manifest} nodeId="node-1" onChange={vi.fn()} onDelete={vi.fn()} onRun={vi.fn()} resources={{}} /></ToastProvider></QueryClientProvider>)

    expect(screen.getByTestId('node-details-view')).toHaveClass('w-[480px]')
    const tabs = [/Parameters|参数/, /Input|输入/, /Output|输出/, /Trace/]
    for (const name of tabs) expect(screen.getByRole('tab', { name })).toBeInTheDocument()
    expect(screen.getByRole('tabpanel', { name: /Parameters|参数/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /More node actions|更多节点操作/ })).toHaveAttribute('aria-haspopup', 'menu')
  })

  it('presents Agent prompt and one user question before an inline advanced section', () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><ToastProvider><NodeInspector data={{ editorKind: 'action', nodeType: 'agent', typeVersion: 1, label: 'Agent', key: 'agent', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }} manifest={agentManifest} nodeId="agent-1" onChange={vi.fn()} onDelete={vi.fn()} resources={{}} /></ToastProvider></QueryClientProvider>)

    expect(screen.getByTestId('parameter-systemPrompt')).toBeInTheDocument()
    expect(screen.getByTestId('parameter-userQuestion').querySelectorAll('input')).toHaveLength(1)
    expect(screen.getByText('Advanced configuration')).toBeInTheDocument()
    expect(screen.getByTestId('parameter-maxIterations')).toBeInTheDocument()
    expect(screen.getByText('calls')).toBeInTheDocument()
    expect(screen.queryByText(/Unsupported UI control|不支持的 UI 控件/)).not.toBeInTheDocument()
  })

  it('configures Agent model, optional Workspace Sandbox and explicit Session Policy in the Inspector', () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const onChange = vi.fn()
    const model = { resourceType: 'model' as const, resourceId: 'model-1', resourceVersionId: 'model-version-1', operation: 'use' as const }
    const sandbox = { resourceType: 'sandbox_profile' as const, resourceId: 'sandbox-1', resourceVersionId: 'sandbox-version-1', operation: 'use' as const }
    const data = { editorKind: 'action' as const, nodeType: 'agent', typeVersion: 2, label: 'Agent', key: 'agent', parameters: { sessionPolicy: { mode: 'invocation' } }, outputProjection: {}, contextWrites: [], resourceReferences: [model, sandbox], settings: {}, disabled: false }
    const view = render(<QueryClientProvider client={client}><ToastProvider><NodeInspector data={data} manifest={agentManifest} nodeId="agent-1" onChange={onChange} onDelete={vi.fn()} resources={{ model: [{ value: 'model-1', label: 'Model 1', resourceType: 'model', operation: 'use', versionId: 'model-version-1', accessState: 'authorized' }], sandbox_profile: [{ value: 'sandbox-1', label: 'Sandbox 1', resourceType: 'sandbox_profile', operation: 'use', versionId: 'sandbox-version-1', accessState: 'authorized' }] }} /></ToastProvider></QueryClientProvider>)

    expect(screen.getByTestId('agent-core-configuration')).toBeInTheDocument()
    expect(screen.getByTestId('agent-inspector-model')).toHaveTextContent(/Model|模型/)
    expect(screen.getByTestId('agent-inspector-workspace_sandbox')).toHaveTextContent(/sandbox/i)
    expect(screen.queryByText(/read.*write.*edit.*bash/i)).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: /Clear resource|清除资源/ }))
    expect(onChange).toHaveBeenCalledWith({ resourceReferences: [model] })

    view.rerender(<QueryClientProvider client={client}><ToastProvider><NodeInspector data={{ ...data, resourceReferences: [model] }} manifest={agentManifest} nodeId="agent-1" onChange={onChange} onDelete={vi.fn()} resources={{}} /></ToastProvider></QueryClientProvider>)
    expect(screen.getByText(/read.*write.*edit.*bash/i)).toBeInTheDocument()
  })

  it('moves node disablement into the more menu and shows disabled status in the header', () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const onChange = vi.fn()
    render(<QueryClientProvider client={client}><ToastProvider><NodeInspector data={{ editorKind: 'action', nodeType: 'set', typeVersion: 1, label: 'Set', key: 'set', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: true }} manifest={manifest} nodeId="node-1" onChange={onChange} onDelete={vi.fn()} resources={{}} /></ToastProvider></QueryClientProvider>)

    expect(screen.getByText(/Disabled|禁用/)).toBeInTheDocument()
    expect(screen.queryByRole('checkbox', { name: /Disabled|禁用/ })).not.toBeInTheDocument()
    fireEvent.pointerDown(screen.getByRole('button', { name: /More node actions|更多节点操作/ }), { button: 0, ctrlKey: false })
    fireEvent.click(screen.getByRole('menuitem', { name: /Enable node|启用节点/ }))
    expect(onChange).toHaveBeenCalledWith({ disabled: false })
  })

  it('shows Model prompt and one user question without the raw parameters field', () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><ToastProvider><NodeInspector data={{ editorKind: 'action', nodeType: 'model', typeVersion: 1, label: 'Model', key: 'model', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }} manifest={modelManifest} nodeId="model-1" onChange={vi.fn()} onDelete={vi.fn()} resources={{}} /></ToastProvider></QueryClientProvider>)

    const prompt = screen.getByTestId('parameter-prompt')
    const question = screen.getByTestId('parameter-userQuestion')
    expect(prompt.compareDocumentPosition(question) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(question.querySelectorAll('input')).toHaveLength(1)
    expect(screen.queryByTestId('parameter-parameters')).not.toBeInTheDocument()
  })

  it('exposes the current input item to node parameter expressions', () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const referenceCatalog = { inputs: [], outputs: [], contexts: [], item: [] }
    const condition = { kind: 'expression', root: { kind: 'reference', selector: { namespace: 'inputs', run: { kind: 'current' }, item: { kind: 'current' }, path: [] }, missingPolicy: { kind: 'error' } } }
    render(<QueryClientProvider client={client}><ToastProvider><NodeInspector data={{ editorKind: 'action', nodeType: 'filter', typeVersion: 1, label: 'Filter', key: 'filter', parameters: { condition }, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }} manifest={filterManifest} nodeId="filter-1" onChange={vi.fn()} onDelete={vi.fn()} referenceCatalog={referenceCatalog} resources={{}} /></ToastProvider></QueryClientProvider>)

    fireEvent.click(screen.getByRole('button', { name: /inputs|选择变量/i }))

    expect(screen.getByRole('button', { name: /Current data|当前数据/ })).toBeInTheDocument()
  })

  it('configures custom outputs and context writes through dialogs', () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const onChange = vi.fn()
    const referenceCatalog = { inputs: [], outputs: [], contexts: [{ id: 'contexts.session', label: 'session', path: 'contexts.session', type: 'object', children: [{ id: 'contexts.session.answer', label: 'answer', path: 'contexts.session.answer', selector: { namespace: 'contexts' as const, run: { kind: 'current' as const }, item: { kind: 'current' as const }, path: ['session', 'answer'] }, type: 'string', children: [] }] }] }
    render(<QueryClientProvider client={client}><ToastProvider><NodeInspector data={{ editorKind: 'action', nodeType: 'model', typeVersion: 1, label: 'Model', key: 'model', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }} manifest={modelManifest} nodeId="model-1" onChange={onChange} onDelete={vi.fn()} referenceCatalog={referenceCatalog} resources={{}} /></ToastProvider></QueryClientProvider>)

    fireEvent.click(screen.getByRole('button', { name: /Add custom output|添加自定义输出/ }))
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    fireEvent.change(screen.getByLabelText(/Output name|输出名称/), { target: { value: 'answer' } })
    fireEvent.click(screen.getByRole('button', { name: /Save|保存/ }))
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ outputProjection: expect.objectContaining({ main: expect.objectContaining({ answer: expect.objectContaining({ value: { kind: 'literal', value: '' } }) }) }) }))

    fireEvent.click(screen.getByRole('button', { name: /Add global variable write|添加全局变量写入/ }))
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('combobox', { name: /Global variable|全局变量/ }))
    fireEvent.click(screen.getByRole('option', { name: 'session.answer' }))
    fireEvent.click(screen.getByRole('button', { name: /Save|保存/ }))
    expect(onChange).toHaveBeenCalledWith({ contextWrites: [{ operation: 'set', path: 'session.answer', value: { kind: 'literal', value: '' } }] })
  })

  it('offers execution information in output projection and context write dialogs', () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(<QueryClientProvider client={client}><ToastProvider><NodeInspector data={{ editorKind: 'action', nodeType: 'model', typeVersion: 1, label: 'Model', key: 'model', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }} manifest={modelManifest} nodeId="model-1" onChange={vi.fn()} onDelete={vi.fn()} referenceCatalog={executionCatalog} resources={{}} /></ToastProvider></QueryClientProvider>)

    fireEvent.click(screen.getByRole('button', { name: /Add custom output|添加自定义输出/ }))
    let dialog = screen.getByRole('dialog', { name: /Add custom output|添加自定义输出/ })
    fireEvent.focus(within(dialog).getByLabelText('Value'))
    expect(screen.getByRole('button', { name: /Execution information|运行信息/ })).toBeInTheDocument()
    fireEvent.click(within(dialog).getByRole('button', { name: /Cancel|取消/ }))

    fireEvent.click(screen.getByRole('button', { name: /Add global variable write|添加全局变量写入/ }))
    dialog = screen.getByRole('dialog', { name: /Add global variable write|添加全局变量写入/ })
    fireEvent.focus(within(dialog).getByLabelText('Value'))
    expect(screen.getByRole('button', { name: /Execution information|运行信息/ })).toBeInTheDocument()
  })

  it('rejects duplicate projection field names before overwriting an existing field', () => {
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const onChange = vi.fn()
    const outputProjection = { main: { answer: { value: { kind: 'literal' as const, value: '' }, schema: { type: 'string' }, sensitive: false } } }
    render(<QueryClientProvider client={client}><ToastProvider><NodeInspector data={{ editorKind: 'action', nodeType: 'model', typeVersion: 1, label: 'Model', key: 'model', parameters: {}, outputProjection, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }} manifest={modelManifest} nodeId="model-1" onChange={onChange} onDelete={vi.fn()} resources={{}} /></ToastProvider></QueryClientProvider>)

    fireEvent.click(screen.getByRole('button', { name: /Add custom output|添加自定义输出/ }))
    fireEvent.change(screen.getByLabelText(/Output name|输出名称/), { target: { value: 'answer' } })

    expect(screen.getByRole('alert')).toHaveTextContent(/already in use|已被使用/i)
    expect(screen.getByRole('button', { name: /Save|保存/ })).toBeDisabled()
    expect(onChange).not.toHaveBeenCalled()
  })

  it('uses the shared Span query to show the selected node Trace detail', async () => {
    const nativeUrl = URL
    class DownloadUrl extends nativeUrl {
      static createObjectURL = vi.fn(() => 'blob:node-trace-artifact')
      static revokeObjectURL = vi.fn()
    }
    vi.stubGlobal('URL', DownloadUrl)
    const anchorClick = vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(() => undefined)
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const url = String(input)
      if (url.includes('/artifacts/artifact-1')) return new Response('{"trace":"large"}', { headers: { 'Content-Type': 'application/json' } })
      const body = url.endsWith('/nodes')
        ? { items: [{ id: 'node-execution-1', executionId: 'execution-1', nodeId: 'node-1', nodeName: 'Selected Set node', nodeType: 'set', nodeVersion: 1, runIndex: 0, iterationIndex: 0, status: 'succeeded', capability: 'builtin', sideEffectLevel: 'none', input: { main: [{ json: { source: true } }] }, output: { main: [{ json: { value: 1 } }] }, startedAt: '2026-08-19T00:00:00Z', endedAt: '2026-08-19T00:00:01Z', attempts: [], lineage: [] }] }
        : url.includes('/trace/spans/span-attempt-1')
          ? { executionId: 'execution-1', traceId: 'trace-1', span: { spanId: 'span-attempt-1', parentSpanId: 'span-node-1', spanKind: 'attempt', spanName: 'Attempt 1', status: 'succeeded', startedAt: '2026-08-19T00:00:00Z', endedAt: '2026-08-19T00:00:01Z', durationMs: 1000, costMicros: 0, hasDetails: true, nodeExecutionId: 'node-execution-1' }, contents: [{ eventId: 'event-1', kind: 'resolved_parameters', preview: { value: 1 }, contentRef: 'artifact-1', occurredAt: '2026-08-19T00:00:00Z' }], events: [] }
          : { executionId: 'execution-1', traceId: 'trace-1', expectedWatermark: 3, ingestedWatermark: 3, complete: true, degraded: false, warningCode: null, totalSpans: 2, nextCursor: null, spans: [{ spanId: 'span-node-1', parentSpanId: null, spanKind: 'node', spanName: 'Selected Set node', status: 'succeeded', startedAt: '2026-08-19T00:00:00Z', endedAt: '2026-08-19T00:00:01Z', durationMs: 1000, costMicros: 0, hasDetails: false, nodeExecutionId: 'node-execution-1' }, { spanId: 'span-attempt-1', parentSpanId: 'span-node-1', spanKind: 'attempt', spanName: 'Attempt 1', status: 'succeeded', startedAt: '2026-08-19T00:00:00Z', endedAt: '2026-08-19T00:00:01Z', durationMs: 1000, costMicros: 0, hasDetails: true, nodeExecutionId: 'node-execution-1' }] }
      return new Response(JSON.stringify(body), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const data = { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Set', key: 'set', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }
    render(<QueryClientProvider client={client}><ToastProvider><NodeInspector data={data} executionId="execution-1" manifest={manifest} nodeId="node-1" onChange={vi.fn()} onDelete={vi.fn()} resources={{}} /></ToastProvider></QueryClientProvider>)

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('/nodes'), expect.anything()))
    const traceTab = screen.getByRole('tab', { name: 'Trace' })
    screen.getByRole('tab', { name: /Parameters|参数/ }).focus()
    fireEvent.keyDown(screen.getByRole('tablist'), { key: 'End' })
    fireEvent.keyDown(traceTab, { key: 'Enter' })
    await waitFor(() => expect(traceTab).toHaveAttribute('data-state', 'active'))
    expect(await screen.findByText(/Input and resolved parameters|输入与解析参数/)).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('nodeExecutionId=node-execution-1'), expect.anything())
    fireEvent.click(await screen.findByRole('button', { name: /Attempt 1/ }))
    fireEvent.click(await screen.findByRole('button', { name: /artifact/i }))
    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(expect.stringContaining('/executions/execution-1/artifacts/artifact-1'), expect.anything()))
    expect(DownloadUrl.createObjectURL).toHaveBeenCalled()
    expect(anchorClick).toHaveBeenCalled()
    anchorClick.mockRestore()
    vi.unstubAllGlobals()
  })

  it('does not pin the previous node output while the selected node is loading', async () => {
    let resolveCodeRuns: ((response: Response) => void) | undefined
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const url = String(input)
      if (!url.endsWith('/nodes')) return new Response(JSON.stringify({}), { headers: { 'Content-Type': 'application/json' } })
      if (fetchMock.mock.calls.length === 1) {
        return new Response(JSON.stringify({ items: [{ id: 'agent-run', nodeId: 'agent-1', output: { main: [{ json: { text: 'agent-output' } }] } }] }), { headers: { 'Content-Type': 'application/json' } })
      }
      return new Promise<Response>((resolve) => {
        resolveCodeRuns = resolve
      })
    })
    vi.stubGlobal('fetch', fetchMock)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const agentData = { editorKind: 'action' as const, nodeType: 'agent', typeVersion: 1, label: 'Agent', key: 'agent', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }
    const codeData = { ...agentData, nodeType: 'set', label: 'Code', key: 'code' }
    const view = render(<QueryClientProvider client={client}><ToastProvider><NodeInspector data={agentData} executionId="execution-1" manifest={agentManifest} nodeId="agent-1" onChange={vi.fn()} onDelete={vi.fn()} resources={{}} workflowId="workflow-1" /></ToastProvider></QueryClientProvider>)

    await waitFor(() => expect(client.getQueryData(['studio-node-inspector', 'execution-1', 'agent-1'])).toBeTruthy())
    let outputTab = screen.getByRole('tab', { name: /Output|输出/ })
    outputTab.focus()
    fireEvent.keyDown(outputTab, { key: 'Enter' })
    await waitFor(() => expect((screen.getByRole('textbox', { name: /Output|输出/ }) as HTMLTextAreaElement).value).toContain('agent-output'))
    view.rerender(<QueryClientProvider client={client}><ToastProvider><NodeInspector data={codeData} executionId="execution-1" manifest={manifest} nodeId="code-1" onChange={vi.fn()} onDelete={vi.fn()} resources={{}} workflowId="workflow-1" /></ToastProvider></QueryClientProvider>)

    await waitFor(() => expect(screen.getByRole('tab', { name: /Output|输出/ })).toHaveAttribute('data-state', 'inactive'))
    outputTab = screen.getByRole('tab', { name: /Output|输出/ })
    outputTab.focus()
    fireEvent.keyDown(outputTab, { key: 'Enter' })
    await waitFor(() => expect(screen.getByRole('textbox', { name: /Output|输出/ })).toHaveValue('{}'))
    expect(screen.getByRole('button', { name: /Pin|固定/ })).toBeDisabled()

    resolveCodeRuns?.(new Response(JSON.stringify({ items: [{ id: 'code-run', nodeId: 'code-1', output: { main: [{ json: { stdout: 'code-output' } }] } }] }), { headers: { 'Content-Type': 'application/json' } }))
    await waitFor(() => expect((screen.getByRole('textbox', { name: /Output|输出/ }) as HTMLTextAreaElement).value).toContain('code-output'))
    expect(screen.getByRole('button', { name: /Pin|固定/ })).toBeEnabled()
    vi.unstubAllGlobals()
  })

  it('refreshes the selected node until its output is available to pin', async () => {
    let nodePolls = 0
    const fetchMock = vi.fn(async (input: string | URL | Request) => {
      const url = String(input)
      if (!url.endsWith('/nodes')) return new Response(JSON.stringify({}), { headers: { 'Content-Type': 'application/json' } })
      nodePolls += 1
      const output = nodePolls > 1 ? { main: [{ json: { stdout: 'completed-code-output' } }] } : undefined
      return new Response(JSON.stringify({ items: [{ id: 'code-run', nodeId: 'code-1', output }] }), { headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const codeData = { editorKind: 'action' as const, nodeType: 'set', typeVersion: 1, label: 'Code', key: 'code', parameters: {}, outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false }
    const view = render(<QueryClientProvider client={client}><ToastProvider><NodeInspector data={codeData} executionId="execution-1" manifest={manifest} nodeId="code-1" onChange={vi.fn()} onDelete={vi.fn()} resources={{}} workflowId="workflow-1" /></ToastProvider></QueryClientProvider>)

    const outputTab = screen.getByRole('tab', { name: /Output|输出/ })
    outputTab.focus()
    fireEvent.keyDown(outputTab, { key: 'Enter' })
    await waitFor(() => expect(screen.getByRole('button', { name: /Pin|固定/ })).toBeDisabled())
    await waitFor(() => expect((screen.getByRole('textbox', { name: /Output|输出/ }) as HTMLTextAreaElement).value).toContain('completed-code-output'), { timeout: 3_000 })
    expect(nodePolls).toBeGreaterThanOrEqual(2)
    expect(screen.getByRole('button', { name: /Pin|固定/ })).toBeEnabled()

    view.unmount()
    client.clear()
    vi.unstubAllGlobals()
  })
})
