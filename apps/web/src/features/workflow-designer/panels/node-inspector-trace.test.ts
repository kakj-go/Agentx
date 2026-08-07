import { describe, expect, it } from 'vitest'

import type { NodeExecution, RuntimeDetails, Trace } from '../../../shared/api/types'
import { nodeTraceView } from '../utils/node-trace'

describe('nodeTraceView', () => {
  it('keeps attempts, lineage and runtime records related to the selected node only', () => {
    const selected = { id: 'run-a', nodeId: 'node-a', input: { secret: 'not copied' }, output: { large: true }, attempts: [{ id: 'attempt-a' }], lineage: [{ deliveryId: 'delivery-a' }] } as unknown as NodeExecution
    const details = {
      agentRuns: [{ id: 'agent-a', nodeExecutionId: 'run-a' }, { id: 'agent-b', nodeExecutionId: 'run-b' }],
      iterations: [{ id: 'iteration-a', agentRunId: 'agent-a' }, { id: 'iteration-b', agentRunId: 'agent-b' }],
      calls: [{ id: 'call-a', attemptId: 'attempt-a' }, { id: 'call-b', attemptId: 'attempt-b' }],
      sandboxes: [{ id: 'sandbox-a', nodeExecutionId: 'run-a', attemptId: 'attempt-a' }, { id: 'sandbox-b', nodeExecutionId: 'run-b', attemptId: 'attempt-b' }],
    } as unknown as RuntimeDetails
    const trace = { events: [{ eventId: 'event-a', nodeId: 'node-a' }, { eventId: 'event-b', nodeId: 'node-b' }] } as unknown as Trace

    const result = nodeTraceView('node-a', [selected], details, trace)

    expect(result.runs[0]).not.toHaveProperty('input')
    expect(result.runs[0]).not.toHaveProperty('output')
    expect(result.runs[0]).toMatchObject({ id: 'run-a', attempts: [{ id: 'attempt-a' }], lineage: [{ deliveryId: 'delivery-a' }] })
    expect(result.agentRuns.map((item) => item.id)).toEqual(['agent-a'])
    expect(result.iterations.map((item) => item.id)).toEqual(['iteration-a'])
    expect(result.calls.map((item) => item.id)).toEqual(['call-a'])
    expect(result.sandboxes.map((item) => item.id)).toEqual(['sandbox-a'])
    expect(result.traceEvents.map((item) => item.eventId)).toEqual(['event-a'])
  })
})
