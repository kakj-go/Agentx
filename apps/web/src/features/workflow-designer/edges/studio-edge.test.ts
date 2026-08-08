import { Position } from '@xyflow/react'
import { describe, expect, it } from 'vitest'

import { getStudioEdgePath } from './studio-edge'

describe('studio edge routing', () => {
  it('uses the n8n-style 130px lower lane for backward execution edges', () => {
    const result = getStudioEdgePath({ sourceX: 420, sourceY: 120, sourcePosition: Position.Right, targetX: 120, targetY: 80, targetPosition: Position.Left })
    expect(result.labelX).toBe(270)
    expect(result.labelY).toBe(250)
    expect(result.path).toContain('250')
  })

  it('keeps binding and forward edges on Bezier paths', () => {
    const binding = getStudioEdgePath({ sourceX: 120, sourceY: 220, sourcePosition: Position.Top, targetX: 200, targetY: 100, targetPosition: Position.Bottom }, true)
    const forward = getStudioEdgePath({ sourceX: 120, sourceY: 100, sourcePosition: Position.Right, targetX: 320, targetY: 100, targetPosition: Position.Left })
    expect(binding.path.startsWith('M')).toBe(true)
    expect(forward.labelY).toBe(100)
  })
})
