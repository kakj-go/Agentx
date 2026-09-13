import { schemaDefaults, type JsonSchema } from '../../shared/components/schema-form'

export function projectHistoricalInput(schema: JsonSchema, input: unknown): Record<string, unknown> {
  const projected = schemaDefaults(schema)
  if (!isRecord(input)) return projected
  for (const [name, field] of Object.entries(schema.properties ?? {})) {
    if (!Object.prototype.hasOwnProperty.call(input, name)) continue
    const value = projectValue(field, input[name])
    if (value.matched) projected[name] = value.value
  }
  return projected
}

type ProjectedValue = { matched: true; value: unknown } | { matched: false }

function projectValue(schema: JsonSchema, value: unknown): ProjectedValue {
  if (schema.enum?.length && !schema.enum.some((item) => JSON.stringify(item) === JSON.stringify(value))) return { matched: false }
  if (schema['x-agentx-artifact']) {
    const valid = schema['x-agentx-artifact-array'] ? Array.isArray(value) && value.every(isRecord) : isRecord(value)
    return valid ? { matched: true, value: structuredClone(value) } : { matched: false }
  }
  if (schema.type === 'string') return typeof value === 'string' ? { matched: true, value } : { matched: false }
  if (schema.type === 'boolean') return typeof value === 'boolean' ? { matched: true, value } : { matched: false }
  if (schema.type === 'number') return typeof value === 'number' && Number.isFinite(value) ? { matched: true, value } : { matched: false }
  if (schema.type === 'integer') return typeof value === 'number' && Number.isInteger(value) ? { matched: true, value } : { matched: false }
  if (schema.type === 'array') {
    const items = schema.items
    if (!Array.isArray(value) || (items && !value.every((item) => projectValue(items, item).matched))) return { matched: false }
    return { matched: true, value: structuredClone(value) }
  }
  if (schema.type === 'object') {
    if (!isRecord(value)) return { matched: false }
    if (!schema.properties) return { matched: true, value: structuredClone(value) }
    return { matched: true, value: projectHistoricalInput(schema, value) }
  }
  return value === undefined ? { matched: false } : { matched: true, value: structuredClone(value) }
}

function isRecord(value: unknown): value is Record<string, unknown> { return Boolean(value) && typeof value === 'object' && !Array.isArray(value) }
