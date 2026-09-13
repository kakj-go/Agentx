import type { Execute, Json } from '@agentx/plugin-sdk'

export const execute: Execute<{ url: string }> = async (context) => context.trace.span('Fetch customer', async (span) => {
  const response = await context.http({ method: 'GET', url: context.parameters.url })
  span.setAttribute('status', response.status)
  span.content({ type: 'example/http-summary', version: 1, data: { status: response.status } })
  return { status: 'completed', outputs: { main: [{ json: response as unknown as Json }] } }
})
