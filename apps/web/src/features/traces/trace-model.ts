import type { TraceSpan } from '../../shared/api/types'

export type TraceRow = { span: TraceSpan; depth: number }

export function buildTraceRows(spans: TraceSpan[], collapsed: Set<string>, search = '', kind = 'all', errorsOnly = false): TraceRow[] {
  const byId = new Map(spans.map((span) => [span.spanId, span]))
  const matching = new Set(spans.filter((span) => (kind === 'all' || span.spanKind === kind) && (!errorsOnly || isError(span.status)) && (!search || `${span.spanName} ${span.spanId} ${span.errorCode ?? ''} ${span.errorMessage ?? ''}`.toLowerCase().includes(search.toLowerCase()))).map((span) => span.spanId))
  if (search || kind !== 'all' || errorsOnly) for (const id of [...matching]) { let parent = byId.get(id)?.parentSpanId; while (parent && byId.has(parent)) { matching.add(parent); parent = byId.get(parent)?.parentSpanId } }
  const children = new Map<string | null, TraceSpan[]>()
  for (const span of spans) { if (!matching.has(span.spanId)) continue; const parent = span.parentSpanId && byId.has(span.parentSpanId) ? span.parentSpanId : null; const list = children.get(parent) ?? []; list.push(span); children.set(parent, list) }
  for (const list of children.values()) list.sort((a, b) => new Date(a.startedAt).getTime() - new Date(b.startedAt).getTime() || a.spanId.localeCompare(b.spanId))
  const rows: TraceRow[] = []
  const visit = (parent: string | null, depth: number) => { for (const span of children.get(parent) ?? []) { rows.push({ span, depth }); if (!collapsed.has(span.spanId)) visit(span.spanId, depth + 1) } }
  visit(null, 0)
  return rows
}

export function traceBounds(spans: TraceSpan[], now: number) { const starts = spans.map((span) => new Date(span.startedAt).getTime()).filter(Number.isFinite); const start = starts.length ? Math.min(...starts) : now; const ends = spans.map((span) => span.endedAt ? new Date(span.endedAt).getTime() : now).filter(Number.isFinite); return { start, duration: Math.max(1, (ends.length ? Math.max(...ends) : now) - start) } }
export function toggleSet(current: Set<string>, id: string) { const next = new Set(current); if (next.has(id)) next.delete(id); else next.add(id); return next }
export function isError(status: string) { return ['failed', 'timed_out', 'outcome_unknown'].includes(status) }
export function duration(value?: number | null) { if (value == null) return '—'; if (value < 1000) return `${value} ms`; if (value < 60_000) return `${(value / 1000).toFixed(2)} s`; return `${Math.floor(value / 60_000)}m ${((value % 60_000) / 1000).toFixed(1)}s` }
export function statusTone(status: string): 'success' | 'danger' | 'warning' | 'primary' | 'neutral' { if (status === 'succeeded' || status === 'completed') return 'success'; if (isError(status)) return 'danger'; if (status === 'waiting' || status === 'queued' || status === 'reserved') return 'warning'; if (status === 'running' || status === 'sent') return 'primary'; return 'neutral' }
export const spanKinds = ['execution', 'boundary', 'node', 'attempt', 'agent_run', 'agent_iteration', 'runtime_call', 'sandbox', 'wait']
export const kindColor: Record<string, string> = { execution: 'bg-primary', boundary: 'bg-fuchsia-500', node: 'bg-blue-500', attempt: 'bg-cyan-500', agent_run: 'bg-violet-500', agent_iteration: 'bg-purple-400', runtime_call: 'bg-amber-500', sandbox: 'bg-emerald-500', wait: 'bg-orange-500' }
