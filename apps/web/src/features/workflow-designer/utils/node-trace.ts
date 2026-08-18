import type { NodeExecution, RuntimeDetails, Trace } from '../../../shared/api/types'

export function nodeTraceView(nodeId: string | undefined, runs: NodeExecution[], details?: RuntimeDetails, trace?: Trace) {
  const nodeExecutionIds = new Set(runs.map((run) => run.id))
  const attemptIds = new Set(runs.flatMap((run) => (run.attempts ?? []).map((attempt) => attempt.id)))
  const agentRuns = (details?.agentRuns ?? []).filter((run) => nodeExecutionIds.has(run.nodeExecutionId))
  const agentRunIds = new Set(agentRuns.map((run) => run.id))
  return {
    nodeId,
    runs: runs.map(({ input: _input, output: _output, ...run }) => run),
    agentRuns,
    iterations: (details?.iterations ?? []).filter((iteration) => agentRunIds.has(iteration.agentRunId)),
    calls: (details?.calls ?? []).filter((call) => attemptIds.has(call.attemptId) || Boolean(call.agentRunId && agentRunIds.has(call.agentRunId))),
    sandboxes: (details?.sandboxes ?? []).filter((sandbox) => nodeExecutionIds.has(sandbox.nodeExecutionId) || attemptIds.has(sandbox.attemptId)),
    traceEvents: (trace?.events ?? []).filter((event) => event.nodeId === nodeId || Boolean(event.nodeExecutionId && nodeExecutionIds.has(event.nodeExecutionId)) || Boolean(event.attemptId && attemptIds.has(event.attemptId))),
  }
}
