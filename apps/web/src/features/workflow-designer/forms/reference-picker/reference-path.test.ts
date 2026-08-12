import { describe, expect, it } from 'vitest'

import type { NodeManifest, StudioDocument, StudioNode } from '../../model/types'
import { insertAtSelection } from './reference-insertion'
import { buildReferenceCatalog } from './reference-path'

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

describe('Workflow 4.0 reference paths', () => {
  it('builds the three namespaces and filters outputs to reachable predecessors', () => {
    const catalog = buildReferenceCatalog(document, new Map([['if@1', manifest]]), 'target')

    expect(catalog.inputs[0].expression).toBe('${{ inputs.question }}')
    expect(catalog.outputs.map((entry) => entry.label)).toEqual(['condition'])
    expect(catalog.contexts[0].children[0].sensitive).toBe(true)
  })

  it('quotes keyword ports and non-identifier fields and keeps explicit item selectors', () => {
    const catalog = buildReferenceCatalog(document, new Map([['if@1', manifest]]), 'target')
    const port = catalog.outputs[0].children[0]
    const currentField = port.children.find((entry) => entry.label === 'current')!.children[0]
    const all = port.children.find((entry) => entry.label === 'all()')!

    expect(currentField.expression).toBe('${{ outputs.condition["true"].current.json["customer-name"] }}')
    expect(all.expression).toBe('${{ outputs.condition["true"].all() }}')
    expect(port.nullable).toBe(true)
    expect(all.nullable).toBe(false)
  })

  it('inserts at the active selection and returns the next cursor position', () => {
    expect(insertAtSelection('ask: old text', '${{ inputs.question }}', 5, 8)).toEqual({
      value: 'ask: ${{ inputs.question }} text',
      cursor: 27,
    })
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

    expect(code.expression).toBe('${{ outputs.child.error.current.json.code }}')
  })
})
