import { describe, expect, it } from 'vitest'

import type { NodeManifest } from '../model/types'
import { canvasNodeMetrics, findOpenCanvasPosition, nodeCategory, nodeShape } from './node-appearance'

const manifest = (overrides: Partial<NodeManifest>): NodeManifest => ({
  protocolVersion: '1.0', nodeType: 'node', version: 1, displayName: 'Node', description: '', category: 'actions', keywords: [], iconKey: 'box', executionStyle: 'action', capability: 'builtin', readiness: 'any', inputPorts: [], outputPorts: [], bindingSlots: [], parameterSchema: {}, uiSchema: {}, providers: [], credentials: [], retryPolicy: { retryable: false, maxAttempts: 1, initialBackoffMs: 0, maxBackoffMs: 0 }, sandboxRequired: false, supportsMock: true, sideEffectLevel: 'none', ...overrides,
})

describe('workflow node appearance', () => {
  it('prioritizes capability-specific groups over the backend category', () => {
    expect(nodeCategory(manifest({ nodeType: 'rag', capability: 'rag', category: 'ai' }))).toBe('data')
    expect(nodeCategory(manifest({ nodeType: 'agent', capability: 'agent', category: 'ai' }))).toBe('ai')
    expect(nodeCategory(manifest({ nodeType: 'manual_trigger', executionStyle: 'trigger' }))).toBe('triggers')
  })

  it('falls back to default when an old Manifest does not declare a role', () => {
    expect(nodeShape(manifest({ executionStyle: 'trigger' }))).toBe('default')
    expect(nodeShape(manifest({ nodeType: 'if', category: 'flow' }))).toBe('default')
    expect(nodeShape(manifest({ capability: 'agent' }))).toBe('default')
    expect(nodeShape(manifest({ capability: 'sandbox' }))).toBe('default')
  })

  it('uses a declared role and shared role metrics', () => {
    expect(nodeShape(manifest({ category: 'actions', uiSchema: { canvas: { role: 'code' } } }))).toBe('code')
    expect(canvasNodeMetrics('trigger')).toMatchObject({ width: 112, height: 96 })
    expect(canvasNodeMetrics('branch')).toMatchObject({ width: 96, height: 96 })
    expect(canvasNodeMetrics('merge')).toMatchObject({ width: 112, height: 88 })
    expect(canvasNodeMetrics('approval')).toMatchObject({ width: 176, height: 88 })
    expect(canvasNodeMetrics('error_handler')).toMatchObject({ width: 144, height: 72 })
    expect(canvasNodeMetrics('agent', { richHeight: 220 })).toMatchObject({ width: 320, height: 220 })
    expect(canvasNodeMetrics('default', { kind: 'binding' })).toMatchObject({ width: 80, height: 80 })
    expect(canvasNodeMetrics('default', { kind: 'group' }, true)).toMatchObject({ width: 240, height: 64 })
    expect(Object.fromEntries((['default', 'flow', 'trigger', 'branch', 'merge', 'loop', 'suspend', 'approval', 'sub_workflow', 'agent', 'code', 'error_handler'] as const).map((role) => [role, canvasNodeMetrics(role)]))).toEqual({
      default: { width: 96, height: 96, labelBelow: true },
      flow: { width: 96, height: 96, labelBelow: true },
      trigger: { width: 112, height: 96, labelBelow: true },
      branch: { width: 96, height: 96, labelBelow: true },
      merge: { width: 112, height: 88, labelBelow: true },
      loop: { width: 112, height: 88, labelBelow: true },
      suspend: { width: 160, height: 72, labelBelow: false },
      approval: { width: 176, height: 88, labelBelow: false },
      sub_workflow: { width: 112, height: 88, labelBelow: true },
      agent: { width: 320, height: 160, labelBelow: false },
      code: { width: 112, height: 88, labelBelow: true },
      error_handler: { width: 144, height: 72, labelBelow: false },
    })
  })

  it('centers a clicked node and moves it to the nearest open position when occupied', () => {
    expect(findOpenCanvasPosition({ x: 500, y: 400 }, { width: 96, height: 96 }, [])).toEqual({ x: 452, y: 352 })
    expect(findOpenCanvasPosition(
      { x: 500, y: 400 },
      { width: 96, height: 96 },
      [{ x: 440, y: 340, width: 112, height: 96 }],
    )).toEqual({ x: 620, y: 352 })
    expect(findOpenCanvasPosition(
      { x: 500, y: 500 },
      { width: 240, height: 160 },
      [{ x: 380, y: 420, width: 240, height: 160 }],
      { x: 0, y: 0, width: 1000, height: 760 },
    )).toEqual({ x: 692, y: 420 })
  })
})
