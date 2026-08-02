import type { ApiError, AuthResponse } from './types'

let accessToken: string | undefined
let refreshHandler: (() => Promise<boolean>) | undefined
let refreshPromise: Promise<boolean> | undefined

export class ApiClientError extends Error {
  status: number
  detail: ApiError
  constructor(status: number, detail: ApiError) { super(detail.message); this.status = status; this.detail = detail }
}

export function setAccessToken(value?: string | null) { accessToken = value ?? undefined }
export function setRefreshHandler(handler: () => Promise<boolean>) { refreshHandler = handler }

async function parseError(response: Response): Promise<ApiError> {
  try { return await response.json() as ApiError } catch { return { code: 'HTTP_ERROR', message: response.statusText, requestId: response.headers.get('x-request-id') ?? '' } }
}

async function send<T>(path: string, init: RequestInit, retry: boolean): Promise<T> {
  const headers = new Headers(init.headers)
  if (init.body) headers.set('Content-Type', 'application/json')
  if (accessToken) headers.set('Authorization', `Bearer ${accessToken}`)
  const response = await fetch(`/api/v1${path}`, { ...init, credentials: 'include', headers })
  if (response.status === 401 && retry && refreshHandler) {
    refreshPromise ??= refreshHandler().finally(() => { refreshPromise = undefined })
    if (await refreshPromise) return send<T>(path, init, false)
  }
  if (!response.ok) throw new ApiClientError(response.status, await parseError(response))
  if (response.status === 204) return undefined as T
  return response.json() as Promise<T>
}

export function apiRequest<T>(path: string, init: RequestInit = {}) { return send<T>(path, init, true) }
export function publicRequest<T>(path: string, init: RequestInit = {}) { return send<T>(path, init, false) }
export const jsonBody = (value: unknown) => JSON.stringify(value)

export async function refreshAccessToken() {
  const response = await publicRequest<AuthResponse>('/auth/refresh', { method: 'POST' })
  setAccessToken(response.accessToken)
  return response
}
