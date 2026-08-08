import { useEffect, useRef, useState } from 'react'

import { apiRequest } from '../../../shared/api/client'
import type { Execution } from '../../../shared/api/types'
import { loadExecutionEvents, type ExecutionEvent } from './studio-api'
import { mergeRuntimeStatuses, RuntimeStatusBatcher } from './runtime-status-batcher'

const terminal = new Set(['succeeded', 'failed', 'cancelled', 'timed_out'])

export function useExecutionEvents(executionId?: string) {
  const [events, setEvents] = useState<ExecutionEvent[]>([])
  const [running, setRunning] = useState(false)
  const [nodeStatuses, setNodeStatuses] = useState<Map<string, string>>(new Map())
  const cursor = useRef(0)

  useEffect(() => {
    setEvents([])
    setNodeStatuses(new Map())
    cursor.current = 0
    setRunning(Boolean(executionId))
    if (!executionId) return
    let cancelled = false
    let timer: number | undefined
    let quietPolls = 0
    const statusBatcher = new RuntimeStatusBatcher((updates) => setNodeStatuses((current) => mergeRuntimeStatuses(current, updates)))
    const mergeNodeStatuses = (items: ExecutionEvent[]) => {
      const updates: Array<[string, string]> = []
      for (const event of items) {
        const summary = event.summary && typeof event.summary === 'object' ? event.summary as Record<string, unknown> : undefined
        const nodeId = summary?.nodeId ?? summary?.node_id
        if (typeof nodeId !== 'string') continue
        updates.push([nodeId, event.status])
      }
      statusBatcher.merge(updates)
    }
    const poll = async () => {
      try {
        const page = await loadExecutionEvents(executionId, cursor.current)
        if (cancelled) return
        if (page.items.length) {
          quietPolls = 0
          cursor.current = page.nextCursor ?? page.items.at(-1)?.sequence ?? cursor.current
          setEvents((current) => [...current, ...page.items.filter((item) => !current.some((value) => value.sequence === item.sequence))].sort((a, b) => a.sequence - b.sequence))
          mergeNodeStatuses(page.items)
        } else quietPolls += 1
        const execution = await apiRequest<Execution>(`/executions/${executionId}`)
        const active = !terminal.has(execution.status)
        setRunning(active)
        if (active) timer = window.setTimeout(poll, quietPolls > 6 ? 2000 : 650)
        else if (page.items.length === 200) timer = window.setTimeout(poll, 0)
      } catch {
        if (!cancelled) timer = window.setTimeout(poll, 2000)
      }
    }
    void poll()
    return () => { cancelled = true; if (timer) window.clearTimeout(timer); statusBatcher.dispose() }
  }, [executionId])
  return { events, nodeStatuses, running, setRunning }
}
