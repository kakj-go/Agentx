import { afterEach, describe, expect, it, vi } from 'vitest'

import { apiRequest, setAccessToken, setRefreshHandler } from './client'

describe('api client', () => {
  afterEach(() => { vi.unstubAllGlobals(); setAccessToken() })

  it('coalesces concurrent unauthorized responses into one refresh', async () => {
    let requests = 0
    vi.stubGlobal('fetch', vi.fn(async () => {
      requests += 1
      if (requests <= 2) return new Response(JSON.stringify({ code: 'EXPIRED', message: 'expired', requestId: 'r' }), { status: 401, headers: { 'Content-Type': 'application/json' } })
      return new Response(JSON.stringify({ ok: true }), { status: 200, headers: { 'Content-Type': 'application/json' } })
    }))
    let refreshes = 0
    setRefreshHandler(async () => { refreshes += 1; await Promise.resolve(); setAccessToken('renewed'); return true })
    const [first, second] = await Promise.all([apiRequest<{ ok: boolean }>('/one'), apiRequest<{ ok: boolean }>('/two')])
    expect(first.ok).toBe(true); expect(second.ok).toBe(true); expect(refreshes).toBe(1)
  })

  it('lets the browser set the multipart boundary for FormData', async () => {
    const fetchMock = vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
      expect(new Headers(init?.headers).has('Content-Type')).toBe(false)
      return new Response(JSON.stringify({ id: 'artifact' }), { status: 200, headers: { 'Content-Type': 'application/json' } })
    })
    vi.stubGlobal('fetch', fetchMock)
    const form = new FormData()
    form.append('file', new Blob(['PK\u0003\u0004'], { type: 'application/zip' }), 'skill.zip')

    await apiRequest('/skills/skill-id/artifact', { method: 'POST', body: form })

    expect(fetchMock).toHaveBeenCalledOnce()
  })
})
