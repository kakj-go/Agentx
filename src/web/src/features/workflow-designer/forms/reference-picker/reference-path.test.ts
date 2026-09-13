import { describe, expect, it } from 'vitest'

import type { NodeManifest, StudioDocument, StudioNode } from '../../model/types'
import { buildReferenceCatalog } from './reference-path'
import { selectorDisplayLabel } from '../variable-token-editor'
import { iterationEndId } from '../../utils/connections'

const action = (id: string, key: string): StudioNode => ({
  id,
  type: 'manifest',
  position: { x: 0, y: 0 },
  data: {
    editorKind: 'action', nodeType: 'if', typeVersion: 1, label: key, key, parameters: {},
    contextWrites: [], resourceReferences: [], settings: {}, disabled: false,
  },
})

const manifest = {
  nodeType: 'if', version: 1,
  outputPorts: [{ name: 'true', kind: 'main', required: false, variadic: false }],
  outputSchema: { type: 'object', properties: { 'customer-name': { type: 'string' } }, required: ['customer-name'] },
  outputCardinality: { true: 'zero_or_many' },
  selectorCapabilities: { supportsCurrent: true, supportsFirstLast: true, supportsAll: true },
} as unknown as NodeManifest

const document = {
  start: {
    inputs: { type: 'object', properties: { question: { type: 'string' } }, required: ['question'] },
    contexts: {
      session: { schema: { type: 'object', properties: { token: { type: 'string' } } }, default: {}, mutable: true, sensitive: true, clientWritable: false, scope: 'session', mergePolicy: 'replace' },
    },
  },
  nodes: [action('first', 'condition'), action('unrelated', 'unrelated'), action('target', 'target')],
  edges: [{ id: 'edge', source: 'first', target: 'target', sourceHandle: 'true', targetHandle: 'main', data: { edgeKind: 'execution' } }],
} as Pick<StudioDocument, 'start' | 'nodes' | 'edges'>

describe('Workflow 5.0 reference selectors', () => {
  it('builds the shared execution catalog with typed role and nullable initiator selectors', () => {
    const catalog = buildReferenceCatalog(document, new Map())
    const root = catalog.execution?.[0]
    const initiator = root?.children.find((entry) => entry.id === 'execution.group.initiator')
    const roleCodes = initiator?.children.find((entry) => entry.path === 'execution.initiator.roles.codes')
    const departmentName = initiator?.children.find((entry) => entry.path === 'execution.initiator.department.name')

    expect(root?.label).toBe('Execution information')
    expect(roleCodes?.type).toBe('array')
    expect(roleCodes?.selector?.path).toEqual(['initiator', 'roles', 'codes'])
    expect(departmentName?.nullable).toBe(true)
    expect(departmentName?.description).toBe('Empty for some trigger types')
  })

  it('uses localized execution paths for chips without persisting display metadata', () => {
    const labels: Record<string, string> = {
      'studio.executionReferences.root': '运行信息',
      'studio.executionReferences.initiator': '发起人',
      'studio.executionReferences.departmentName': '部门名称',
    }
    const catalog = buildReferenceCatalog(document, new Map(), undefined, (key, fallback) => labels[key] ?? fallback)
    const selector = catalog.execution![0].children
      .find((entry) => entry.id === 'execution.group.initiator')!.children
      .find((entry) => entry.path === 'execution.initiator.department.name')!.selector!

    expect(selectorDisplayLabel(selector, catalog)).toBe('运行信息 / 发起人 / 部门名称')
    expect(selector).toEqual({ namespace: 'execution', run: { kind: 'current' }, item: { kind: 'current' }, path: ['initiator', 'department', 'name'] })
  })

  it('builds the three namespaces and filters outputs to reachable predecessors', () => {
    const catalog = buildReferenceCatalog(document, new Map([['if@1', manifest]]), 'target')

    expect(catalog.inputs[0].selector).toEqual({ namespace: 'inputs', run: { kind: 'current' }, item: { kind: 'current' }, path: ['question'] })
    expect(catalog.outputs.map((entry) => entry.label)).toEqual(['condition'])
    expect(catalog.contexts[0].children[0].sensitive).toBe(true)
  })

  it('uses separate virtual predecessor targets for Exit main and error mappings', () => {
    const exitDocument = {
      ...document,
      edges: [
        { id: 'main-edge', source: 'first', target: 'target', sourceHandle: 'true', targetHandle: 'main', data: { edgeKind: 'execution' as const } },
        { id: 'error-edge', source: 'unrelated', target: 'target', sourceHandle: 'true', targetHandle: 'error', data: { edgeKind: 'execution' as const } },
      ],
    }
    const manifests = new Map([['if@1', manifest]])
    const main = buildReferenceCatalog(exitDocument, manifests, 'target', undefined, 'main')
    const error = buildReferenceCatalog(exitDocument, manifests, 'target', undefined, 'error')

    expect(main.outputs.map((entry) => entry.label)).toEqual(['condition'])
    expect(error.outputs.map((entry) => entry.label)).toEqual(['unrelated'])
  })

  it.each(['model', 'code'])('uses the explicit %s structured output contract in the picker', (nodeType) => {
    const outputSchema = { type: 'object', properties: { answer: { type: 'string' } }, required: ['answer'] }
    const source = action('first', nodeType)
    if (source.data.editorKind !== 'action') throw new Error('action fixture required')
    source.data.nodeType = nodeType
    source.data.parameters = nodeType === 'model'
      ? { responseMode: 'json_schema', structuredSchema: outputSchema }
      : { outputExample: { answer: '' } }
    const outputManifest = {
      ...manifest,
      nodeType,
      outputPorts: [{ name: 'main', kind: 'main', required: false, variadic: false }],
      outputSchema: { type: 'object', properties: { structuredOutput: {} } },
    } as NodeManifest
    const catalog = buildReferenceCatalog(
      { ...document, nodes: [source, action('target', 'target')], edges: [{ id: 'edge', source: 'first', target: 'target', sourceHandle: 'main', targetHandle: 'main', data: { edgeKind: 'execution' } }] },
      new Map([[`${nodeType}@1`, outputManifest]]),
      'target',
    )
    const current = catalog.outputs[0].children[0].children.find((entry) => entry.label === 'current')!
    const answer = nodeType === 'code' ? current.children.find((entry) => entry.label === 'answer')! : current.children.find((entry) => entry.label === 'structuredOutput')!.children[0]
    expect(answer.type).toBe('string')
    expect(answer.selector?.path).toEqual(['structuredOutput', 'answer'])
  })

  it('quotes keyword ports and non-identifier fields and keeps explicit item selectors', () => {
    const catalog = buildReferenceCatalog(document, new Map([['if@1', manifest]]), 'target')
    const port = catalog.outputs[0].children[0]
    const currentField = port.children.find((entry) => entry.label === 'current')!.children[0]
    const all = port.children.find((entry) => entry.label === 'all()')!

    expect(currentField.selector).toEqual({ namespace: 'outputs', sourceNodeId: 'first', port: 'true', run: { kind: 'current' }, item: { kind: 'current' }, path: ['customer-name'] })
    expect(all.selector).toEqual({ namespace: 'outputs', sourceNodeId: 'first', port: 'true', run: { kind: 'current' }, item: { kind: 'all' }, path: [] })
    expect(port.nullable).toBe(true)
    expect(all.nullable).toBe(false)
  })

  it('uses a port-specific schema for composite error outputs', () => {
    const composite = {
      ...manifest,
      outputPorts: [
        { name: 'main', kind: 'main', required: false, variadic: false },
        { name: 'error', kind: 'error', required: false, variadic: false },
      ],
      outputPortSchemas: {
        error: { type: 'object', properties: { code: { type: 'string' } } },
      },
    } as NodeManifest
    const source = action('first', 'child')
    const catalog = buildReferenceCatalog(
      { ...document, nodes: [source, action('target', 'target')], edges: [{ id: 'edge', source: 'first', target: 'target', sourceHandle: 'main', targetHandle: 'main', data: { edgeKind: 'execution' } }] },
      new Map([['if@1', composite]]),
      'target',
    )
    const errorPort = catalog.outputs[0].children.find((entry) => entry.label === 'error')!
    const code = errorPort.children.find((entry) => entry.label === 'current')!.children[0]

    expect(code.selector).toEqual({ namespace: 'outputs', sourceNodeId: 'first', port: 'error', run: { kind: 'current' }, item: { kind: 'current' }, path: ['code'] })
  })

  it('expands the decision field from an approval port schema', () => {
    const approval = {
      ...manifest,
      nodeType: 'approval',
      outputPorts: [{ name: 'decision', kind: 'main', required: false, variadic: true }],
      outputPortSchemas: {
        decision: {
          type: 'object',
          properties: { decision: { type: 'string', enum: ['approved'] } },
          required: ['decision'],
        },
      },
      outputCardinality: { decision: 'zero_or_one' },
    } as NodeManifest
    const source = { ...action('first', 'approval'), data: { ...action('first', 'approval').data, nodeType: 'approval' } } as StudioNode
    const catalog = buildReferenceCatalog(
      { ...document, nodes: [source, action('target', 'target')], edges: [{ id: 'edge', source: 'first', target: 'target', sourceHandle: 'decision:approved', targetHandle: 'main', data: { edgeKind: 'execution' } }] },
      new Map([['approval@1', approval]]),
      'target',
    )
    const approved = catalog.outputs[0].children[0]
    const decision = approved.children.find((entry) => entry.label === 'current')!.children.find((entry) => entry.label === 'decision')

    expect(decision?.type).toBe('string')
    expect(decision?.selector).toEqual({ namespace: 'outputs', sourceNodeId: 'first', port: 'decision:approved', run: { kind: 'current' }, item: { kind: 'current' }, path: ['decision'] })
  })

  it('uses the selected immutable Sub-workflow Manifest for output fields', () => {
    const versionId = '018f0000-0000-7000-8000-000000000003'
    const generic = { ...manifest, nodeType: 'sub_workflow', outputPorts: [{ name: 'main', kind: 'main', required: false, variadic: false }], outputSchema: { type: 'object' } } as NodeManifest
    const derived = { ...generic, nodeType: 'workflow.018f0000000070008000000000000003', outputSchema: { type: 'object', required: ['answer'], properties: { answer: { type: 'string' } } } } as NodeManifest
    const source = { ...action('first', 'child'), data: { ...action('first', 'child').data, nodeType: 'sub_workflow', parameters: { workflowVersionId: versionId, inputs: {} } } } as StudioNode
    const catalog = buildReferenceCatalog(
      { ...document, nodes: [source, action('target', 'target')], edges: [{ id: 'edge', source: 'first', target: 'target', sourceHandle: 'main', targetHandle: 'main', data: { edgeKind: 'execution' } }] },
      new Map([['sub_workflow@1', generic], ['workflow.018f0000000070008000000000000003@1', derived]]),
      'target',
    )

    expect(catalog.outputs[0].children[0].children.find((entry) => entry.label === 'current')?.children.map((entry) => entry.label)).toEqual(['answer'])
  })

  it('marks Model and Agent text as the recommended stable output', () => {
    const model = {
      ...manifest,
      nodeType: 'model',
      outputPorts: [{ name: 'main', kind: 'main', required: false, variadic: false }],
      outputSchema: { type: 'object', properties: { text: { type: 'string' }, structuredOutput: { type: ['object', 'null'] } }, required: ['text'] },
      outputCardinality: { main: 'exactly_one' },
    } as NodeManifest
    const source = { ...action('first', 'model'), data: { ...action('first', 'model').data, nodeType: 'model' } } as StudioNode
    const catalog = buildReferenceCatalog(
      { ...document, nodes: [source, action('target', 'target')], edges: [{ id: 'edge', source: 'first', target: 'target', sourceHandle: 'main', targetHandle: 'main', data: { edgeKind: 'execution' } }] },
      new Map([['model@1', model]]),
      'target',
    )
    const current = catalog.outputs[0].children[0].children.find((entry) => entry.label === 'current')!
    expect(current.children.find((entry) => entry.label === 'text')?.recommended).toBe(true)
    expect(current.children.find((entry) => entry.label === 'structuredOutput')?.recommended).not.toBe(true)
  })

  it('derives item and loop.item fields from an array input without legacy loop names', () => {
    const items = { type: 'array', items: { type: 'object', properties: { score: { type: 'number' } }, required: ['score'] } }
    const input = { kind: 'reference', selector: { namespace: 'inputs', run: { kind: 'current' }, item: { kind: 'current' }, path: ['items'] }, missingPolicy: { kind: 'error' } }
    const loop = action('loop', 'loop')
    const list = action('list', 'list')
    if (loop.data.editorKind !== 'action' || list.data.editorKind !== 'action') throw new Error('action fixtures required')
    loop.data.nodeType = 'loop_over_items'
    loop.data.parameters = { input }
    list.data.nodeType = 'list'
    list.data.parameters = { input }
    const body = action('body', 'body')
    if (body.data.editorKind !== 'action') throw new Error('action fixture required')
    body.data.parentId = 'loop'
    const scoped = { ...document, start: { ...document.start, inputs: { type: 'object', properties: { items }, required: ['items'] } }, nodes: [loop, body, list] }

    const loopCatalog = buildReferenceCatalog(scoped, new Map(), 'loop')
    expect(loopCatalog.loop?.map((entry) => entry.path)).toEqual(['loop.item', 'loop.items', 'loop.index'])
    expect(loopCatalog.loop?.[0].children[0].selector).toEqual({ namespace: 'loop', run: { kind: 'current' }, item: { kind: 'current' }, path: ['item', 'score'] })
    expect(loopCatalog.loop?.[1].selector).toEqual({ namespace: 'loop', run: { kind: 'current' }, item: { kind: 'current' }, path: ['items'] })
    expect(buildReferenceCatalog(scoped, new Map(), 'body').loop?.[2].selector?.path).toEqual(['index'])
    expect(buildReferenceCatalog(scoped, new Map(), 'list').item?.[0].selector).toEqual({ namespace: 'item', run: { kind: 'current' }, item: { kind: 'current' }, path: ['score'] })
  })

  it('uses the edit-only loop end as the output selector target', () => {
    const loop = action('loop', 'loop')
    const first = action('first', 'first')
    const last = action('last', 'last')
    for (const node of [loop, first, last]) {
      if (node.data.editorKind !== 'action') throw new Error('action fixture required')
    }
    if (loop.data.editorKind === 'action') loop.data.nodeType = 'loop_over_items'
    if (first.data.editorKind === 'action') first.data.parentId = 'loop'
    if (last.data.editorKind === 'action') last.data.parentId = 'loop'
    const scoped = { ...document, nodes: [loop, first, last], edges: [{ id: 'inside', source: 'first', target: 'last', sourceHandle: 'true', targetHandle: 'main', data: { edgeKind: 'execution' as const } }] }

    const catalog = buildReferenceCatalog(scoped, new Map([['if@1', manifest]]), iterationEndId('loop'))
    expect(catalog.outputs.map((entry) => entry.label)).toEqual(['first', 'last'])
    expect(catalog.loop?.map((entry) => entry.path)).toEqual(['loop.item', 'loop.items', 'loop.index'])
  })

  it('derives List item fields from an upstream Code output schema', () => {
    const code = action('code', 'parse')
    const list = action('list', 'list')
    if (code.data.editorKind !== 'action' || list.data.editorKind !== 'action') throw new Error('action fixtures required')
    code.data.nodeType = 'code'
    code.data.parameters = { outputExample: { items: [{ score: 0 }] } }
    list.data.nodeType = 'list'
    list.data.parameters = { input: { kind: 'reference', selector: { namespace: 'outputs', sourceNodeId: 'code', port: 'main', run: { kind: 'current' }, item: { kind: 'current' }, path: ['structuredOutput', 'items'] }, missingPolicy: { kind: 'error' } } }
    const codeManifest = { ...manifest, nodeType: 'code', outputPorts: [{ name: 'main', kind: 'main', required: false, variadic: false }], outputCardinality: { main: 'exactly_one' }, outputSchema: { type: 'object', properties: { structuredOutput: { type: ['object', 'null'] } } } } as NodeManifest
    const listManifest = { ...manifest, nodeType: 'list', outputPorts: [{ name: 'main', kind: 'main', required: false, variadic: false }] } as NodeManifest
    const scoped = { ...document, nodes: [code, list], edges: [{ id: 'code-list', source: 'code', target: 'list', sourceHandle: 'main', targetHandle: 'main', data: { edgeKind: 'execution' as const } }] }

    const catalog = buildReferenceCatalog(scoped, new Map([['code@1', codeManifest], ['list@1', listManifest]]), 'list')
    expect(catalog.item?.map((entry) => entry.label)).toEqual(['score'])
    expect(catalog.item?.[0].schema).toEqual({ type: 'integer' })
  })
})
