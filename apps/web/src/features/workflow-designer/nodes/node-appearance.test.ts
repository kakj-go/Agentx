import { describe, expect, it } from 'vitest'

import type { NodeManifest } from '../model/types'
import { canvasNodeMetrics, findOpenCanvasPosition, groupColor, nodeGroup, nodeShape } from './node-appearance'

const manifest = (overrides: Partial<NodeManifest>): NodeManifest => ({
  protocolVersion: '2.0', nodeType: 'node', version: 1, displayName: 'Node', description: '', category: 'actions', keywords: [], iconKey: 'box', executionStyle: 'action', capability: 'builtin', readiness: 'any', inputPorts: [], outputPorts: [], bindingSlots: [], parameterSchema: {}, uiSchema: {}, providers: [], credentials: [], retryPolicy: { retryable: false, maxAttempts: 1, initialBackoffMs: 0, maxBackoffMs: 0 }, sandboxRequired: false, supportsMock: true, sideEffectLevel: 'none', ...overrides,
})

describe('workflow node appearance', () => {
  it('maps node types onto the six visual groups with the Dify palette', () => {
    expect(nodeGroup('start')).toBe('start')
    expect(nodeGroup('boundary')).toBe('start')
    expect(nodeGroup('agent')).toBe('ai')
    expect(nodeGroup('model')).toBe('ai')
    expect(nodeGroup('if')).toBe('logic')
    expect(nodeGroup('merge')).toBe('logic')
    expect(nodeGroup('loop_over_items')).toBe('logic')
    expect(nodeGroup('approval')).toBe('logic')
    expect(nodeGroup('code')).toBe('transform')
    expect(nodeGroup('set')).toBe('transform')
    expect(nodeGroup('list')).toBe('transform')
    expect(nodeGroup('declarative_http')).toBe('integrate')
    expect(nodeGroup('sub_workflow')).toBe('integrate')
    expect(nodeGroup('exit')).toBe('output')
    expect(nodeGroup('brand_new_node')).toBe('integrate')
    expect(groupColor('start')).toBe('#155EEF')
    expect(groupColor('ai')).toBe('#6366F1')
    expect(groupColor('logic')).toBe('#06B6D4')
    expect(groupColor('transform')).toBe('#3B82F6')
    expect(groupColor('integrate')).toBe('#8B5CF6')
    expect(groupColor('output')).toBe('#F59E0B')
  })

  it('falls back to default when an old Manifest does not declare a role', () => {
    expect(nodeShape(manifest({ executionStyle: 'trigger' }))).toBe('default')
    expect(nodeShape(manifest({ nodeType: 'if', category: 'flow' }))).toBe('default')
    expect(nodeShape(manifest({ capability: 'agent' }))).toBe('default')
    expect(nodeShape(manifest({ capability: 'sandbox' }))).toBe('default')
  })

  it('uses a declared role and shared 240px card metrics', () => {
    expect(nodeShape(manifest({ category: 'actions', uiSchema: { canvas: { role: 'code' } } }))).toBe('code')
    expect(canvasNodeMetrics('trigger')).toMatchObject({ width: 240, height: 62 })
    expect(canvasNodeMetrics('branch', { bodyRows: 0 })).toMatchObject({ width: 240, height: 44 })
    expect(canvasNodeMetrics('agent', { bodyRows: 2, attachments: true })).toMatchObject({ width: 240, height: 104 })
    expect(canvasNodeMetrics('default', { bodyRows: 8 })).toMatchObject({ width: 240, height: 98 })
    expect(canvasNodeMetrics('default', { kind: 'group' }, true)).toMatchObject({ width: 240, height: 64 })
    expect(Object.fromEntries((['default', 'flow', 'trigger', 'branch', 'merge', 'loop', 'suspend', 'approval', 'sub_workflow', 'agent', 'code'] as const).map((role) => [role, canvasNodeMetrics(role)]))).toEqual({
      default: { width: 240, height: 62, labelBelow: false },
      flow: { width: 240, height: 62, labelBelow: false },
      trigger: { width: 240, height: 62, labelBelow: false },
      branch: { width: 240, height: 62, labelBelow: false },
      merge: { width: 240, height: 62, labelBelow: false },
      loop: { width: 240, height: 62, labelBelow: false },
      suspend: { width: 240, height: 62, labelBelow: false },
      approval: { width: 240, height: 62, labelBelow: false },
      sub_workflow: { width: 240, height: 62, labelBelow: false },
      agent: { width: 240, height: 62, labelBelow: false },
      code: { width: 240, height: 62, labelBelow: false },
    })
  })

  it('sizes branch-row and input-row cards at 34px/26px per row', () => {
    expect(canvasNodeMetrics('branch', { bodyRows: 0, branchRows: 3 })).toMatchObject({ width: 240, height: 146 })
    expect(canvasNodeMetrics('approval', { bodyRows: 0, branchRows: 2 })).toMatchObject({ width: 240, height: 112 })
    expect(canvasNodeMetrics('merge', { bodyRows: 1, inputRows: 3 })).toMatchObject({ width: 240, height: 140 })
    expect(canvasNodeMetrics('merge', { bodyRows: 1, inputRows: 3, attachments: true })).toMatchObject({ width: 240, height: 164 })
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
