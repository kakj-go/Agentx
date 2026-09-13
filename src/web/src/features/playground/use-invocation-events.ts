import { useEffect, useRef } from 'react'

import { gatewayRequestStream } from '../../shared/api/client'

type InvocationEventHandler = (eventType: string, payload: unknown) => void

export function useInvocationEvents(invocationId: string | undefined, active: boolean, onEvent: InvocationEventHandler) {
  const cursor = useRef(0)
  const handler = useRef(onEvent)
  handler.current = onEvent

  useEffect(() => {
    cursor.current = 0
    if (!invocationId || !active) return
    const controller = new AbortController()
    let retry: ReturnType<typeof setTimeout> | undefined

    const connect = async () => {
      try {
        const headers = cursor.current > 0 ? { 'Last-Event-ID': String(cursor.current) } : undefined
        const response = await gatewayRequestStream(`/invocations/${invocationId}/events`, { headers, signal: controller.signal })
        const reader = response.body?.getReader()
        if (!reader) return
        const decoder = new TextDecoder()
        let buffer = ''
        while (!controller.signal.aborted) {
          const { done, value } = await reader.read()
          buffer += decoder.decode(value, { stream: !done })
          const frames = buffer.split(/\r?\n\r?\n/)
          buffer = frames.pop() ?? ''
          for (const frame of frames) {
            let eventType = 'message'
            let payload: unknown = null
            for (const line of frame.split(/\r?\n/)) {
              if (line.startsWith('id:')) cursor.current = Number(line.slice(3).trim()) || cursor.current
              if (line.startsWith('event:')) eventType = line.slice(6).trim()
              if (line.startsWith('data:')) {
                const data = line.slice(5).trim()
                try { payload = JSON.parse(data) } catch { payload = data }
              }
            }
            handler.current(eventType, payload)
          }
          if (done) break
        }
      } catch {
        if (controller.signal.aborted) return
      }
      if (!controller.signal.aborted) retry = setTimeout(connect, 750)
    }
    void connect()
    return () => {
      controller.abort()
      if (retry) clearTimeout(retry)
    }
  }, [active, invocationId])
}
