import { describe, expect, it } from 'vitest'

import type { NodeManifest } from '../model/types'
import { nodeCategory, nodeShape } from './node-appearance'

const manifest = (overrides: Partial<NodeManifest>): NodeManifest => ({
  protocolVersion: '1.0', nodeType: 'node', version: 1, displayName: 'Node', description: '', category: 'actions', keywords: [], iconKey: 'box', executionStyle: 'action', capability: 'builtin', readiness: 'any', inputPorts: [], outputPorts: [], bindingSlots: [], parameterSchema: {}, uiSchema: {}, providers: [], credentials: [], retryPolicy: { retryable: false, maxAttempts: 1, initialBackoffMs: 0, maxBackoffMs: 0 }, sandboxRequired: false, supportsMock: true, sideEffectLevel: 'none', ...overrides,
})

describe('workflow node appearance', () => {
  it('prioritizes capability-specific groups over the backend category', () => {
    expect(nodeCategory(manifest({ nodeType: 'rag', capability: 'rag', category: 'ai' }))).toBe('data')
    expect(nodeCategory(manifest({ nodeType: 'agent', capability: 'agent', category: 'ai' }))).toBe('ai')
    expect(nodeCategory(manifest({ nodeType: 'manual_trigger', executionStyle: 'trigger' }))).toBe('triggers')
  })

  it('assigns distinct shapes to triggers, branches, agents and code', () => {
    expect(nodeShape(manifest({ executionStyle: 'trigger' }))).toBe('trigger')
    expect(nodeShape(manifest({ nodeType: 'if', category: 'flow' }))).toBe('branch')
    expect(nodeShape(manifest({ capability: 'agent' }))).toBe('agent')
    expect(nodeShape(manifest({ capability: 'sandbox' }))).toBe('code')
  })
})
