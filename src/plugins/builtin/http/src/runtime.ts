import type { Execute, HttpRequest, Json } from '@agentx/plugin-sdk'

export const execute: Execute<Record<string, Json>> = async (context) => {
  const request = { ...context.parameters, body: context.parameters.body ?? context.inputs.main?.[0]?.json ?? null, idempotencyKey: context.execution.idempotencyKey } as HttpRequest
  const response = await context.http(request)
  return { status: 'completed', outputs: { main: [{ json: response as unknown as Json }] } }
}
