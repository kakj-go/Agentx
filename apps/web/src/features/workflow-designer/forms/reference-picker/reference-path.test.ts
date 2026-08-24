import { describe, expect, it } from 'vitest'

import type { NodeManifest, StudioDocument, StudioNode } from '../../model/types'
import { buildReferenceCatalog } from './reference-path'
import { selectorDisplayLabel } from '../variable-token-editor'

const action = (id: string, key: string): StudioNode => ({
  id,
  type: 'manifest',
  position: { x: 0, y: 0 },
  data: {
    editorKind: 'action', nodeType: 'if', typeVersion: 1, label: key, key, parameters: {},
    outputProjection: {}, contextWrites: [], resourceReferences: [], settings: {}, disabled: false,
  },
})

const manifest = {
  nodeType: 'if', version: 1,
  outputPorts: [{ name: 'true', kind: 'main', required: false, variadic: false }],
  outputSchema: { type: 'object', properties: { 'customer-name': { type: 'string' } }, required: ['customer-name'] },
  outputCardinality: { true: 'zero_or_many' },
  expressionCapabilities: { supportsCurrent: true, supportsFirstLast: true, supportsAll: true },
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
      outputPorts: [{ name: 'approved', kind: 'main', required: false, variadic: false }],
      outputPortSchemas: {
        approved: {
          type: 'object',
          properties: { decision: { type: 'string', enum: ['approved'] } },
          required: ['decision'],
        },
      },
      outputCardinality: { approved: 'zero_or_one' },
    } as NodeManifest
    const source = { ...action('first', 'approval'), data: { ...action('first', 'approval').data, nodeType: 'approval' } } as StudioNode
    const catalog = buildReferenceCatalog(
      { ...document, nodes: [source, action('target', 'target')], edges: [{ id: 'edge', source: 'first', target: 'target', sourceHandle: 'approved', targetHandle: 'main', data: { edgeKind: 'execution' } }] },
      new Map([['approval@1', approval]]),
      'target',
    )
    const approved = catalog.outputs[0].children[0]
    const decision = approved.children.find((entry) => entry.label === 'current')!.children.find((entry) => entry.label === 'decision')

    expect(decision?.type).toBe('string')
    expect(decision?.selector).toEqual({ namespace: 'outputs', sourceNodeId: 'first', port: 'approved', run: { kind: 'current' }, item: { kind: 'current' }, path: ['decision'] })
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
})
