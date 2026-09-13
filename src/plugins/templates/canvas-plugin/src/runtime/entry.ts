import type { Execute, Json, Provider, ResolveDefinition } from '@agentx/plugin-sdk'

export const resolveDefinition: ResolveDefinition<{ label?: string; includeMetadata?: boolean; largeTrace?: boolean }> = (configuration) => ({
  status: configuration.label === '__invalid__' ? 'invalid' : configuration.label?.trim() ? 'complete' : 'incomplete',
  inputPorts: [{ name: 'main', kind: 'main', required: true, variadic: false }],
  outputPorts: [
    { name: 'main', kind: 'main', required: false, variadic: false },
    ...(configuration.includeMetadata ? [{ name: 'metadata' as const, kind: 'main' as const, required: false, variadic: false }] : []),
    { name: 'error', kind: 'error', required: false, variadic: false },
  ],
  outputSchema: { type: 'object', additionalProperties: true },
  outputPortSchemas: configuration.includeMetadata ? { metadata: { type: 'object', properties: { mapped: { type: 'integer' } }, required: ['mapped'], additionalProperties: false } } : undefined,
  issues: configuration.label === '__invalid__'
    ? [{ path: 'label', code: 'LABEL_INVALID', message: 'The reserved invalid label is rejected' }]
    : configuration.label?.trim() ? [] : [{ path: 'label', code: 'LABEL_REQUIRED', message: 'Label is required' }],
})

const labels: Provider = async ({ search = '', limit = 50, cursor, parameters }, context) => {
  if ((parameters as { hostMode?: string } | undefined)?.hostMode === 'missing-model') {
    await context.model({ prompt: 'Discover provider options' })
  }
  if (search === 'slow') await new Promise((resolve) => setTimeout(resolve, 500))
  const values = ['customer', 'order', 'ticket']
    .filter((value) => value.includes(search.toLowerCase()))
    .map((value) => ({ value, label: value[0].toUpperCase() + value.slice(1) }))
  const offset = Number.parseInt(cursor ?? '0', 10) || 0
  const items = values.slice(offset, offset + limit)
  return { items, nextCursor: offset + items.length < values.length ? String(offset + items.length) : null }
}

export const providers = { labels }

export const execute: Execute<{ label: string; includeMetadata?: boolean; largeTrace?: boolean }> = async (context) => context.trace.span('Map items', async (span) => {
  span.setAttribute('itemCount', context.inputs.main?.length ?? 0)
  span.content({ type: 'acme.json-mapper/summary', version: 1, label: 'started', data: { mapped: 0 } })
  await context.trace.span('Prepare mapping', async (prepare) => prepare.event('mapping.ready', { label: context.parameters.label }))
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(resolve, 500)
    context.signal.addEventListener('abort', () => { clearTimeout(timer); reject(new Error('cancelled')) }, { once: true })
  })
  const outputs = (context.inputs.main ?? []).map((item) => ({ ...item, json: { ...(item.json as Record<string, Json>), label: context.parameters.label } }))
  span.content({ type: 'acme.json-mapper/summary', version: 1, label: 'finished', data: { mapped: outputs.length } })
  span.content({ type: 'acme.json-mapper/table', version: 1, data: { columns: ['label', 'items'], rows: [[context.parameters.label, outputs.length]] } })
  if (context.parameters.largeTrace) span.content({ type: 'acme.json-mapper/details', version: 1, data: { payload: 'x'.repeat(20_000) } })
  return { status: 'completed', outputs: { main: outputs, ...(context.parameters.includeMetadata ? { metadata: [{ json: { mapped: outputs.length } }] } : {}) } }
})
