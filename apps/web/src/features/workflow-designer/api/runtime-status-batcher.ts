const ACTIVE_STATUSES = new Set(['pending', 'queued', 'running', 'waiting'])

export class RuntimeStatusBatcher {
  private readonly pending = new Map<string, string>()
  private readonly commit: (updates: Map<string, string>) => void
  private timer?: number

  constructor(commit: (updates: Map<string, string>) => void) { this.commit = commit }

  merge(updates: Iterable<[string, string]>) {
    for (const [nodeId, status] of updates) {
      if (ACTIVE_STATUSES.has(status)) this.pending.set(nodeId, status)
      else {
        this.pending.delete(nodeId)
        this.commit(new Map([[nodeId, status]]))
      }
    }
    if (this.pending.size && this.timer === undefined) this.timer = window.setTimeout(() => this.flush(), 100)
  }

  dispose() {
    if (this.timer !== undefined) window.clearTimeout(this.timer)
    this.timer = undefined
    this.pending.clear()
  }

  private flush() {
    this.timer = undefined
    if (!this.pending.size) return
    this.commit(new Map(this.pending))
    this.pending.clear()
  }
}

export function mergeRuntimeStatuses(current: Map<string, string>, updates: Map<string, string>) {
  const next = new Map(current)
  for (const [nodeId, status] of updates) next.set(nodeId, status)
  return next
}
