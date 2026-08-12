import { Braces, Brackets, CircleDot } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Badge } from '../ui/badge'

type JsonSchema = {
  allOf?: JsonSchema[]
  anyOf?: JsonSchema[]
  const?: unknown
  default?: unknown
  description?: string
  enum?: unknown[]
  format?: string
  items?: JsonSchema
  maximum?: number
  maxLength?: number
  minimum?: number
  minLength?: number
  oneOf?: JsonSchema[]
  pattern?: string
  properties?: Record<string, JsonSchema>
  required?: string[]
  title?: string
  type?: string | string[]
}

type SchemaRow = {
  depth: number
  name: string
  required: boolean
  schema: JsonSchema
}

export function JsonSchemaViewer({ schema }: { schema: unknown }) {
  const { t } = useTranslation()
  const value = isSchema(schema) ? schema : {}
  const rows = flattenSchema(value)

  if (rows.length === 0) {
    return <div className="grid min-h-28 place-items-center rounded-md border border-dashed border-border bg-canvas/40 px-4 text-center text-xs text-muted-foreground">{t('studio.schema.empty')}</div>
  }

  return <div className="overflow-hidden rounded-md border border-border">
    <div className="grid grid-cols-[minmax(180px,1.2fr)_120px_minmax(180px,1fr)_minmax(180px,1fr)] gap-3 bg-muted/45 px-3 py-2 text-[10px] font-medium uppercase text-muted-foreground">
      <span>{t('studio.schema.field')}</span><span>{t('studio.schema.type')}</span><span>{t('studio.schema.description')}</span><span>{t('studio.schema.constraints')}</span>
    </div>
    <div className="divide-y divide-border">
      {rows.map((row, index) => <SchemaField key={`${row.name}-${row.depth}-${index}`} row={row} />)}
    </div>
  </div>
}

function SchemaField({ row }: { row: SchemaRow }) {
  const { t } = useTranslation()
  const type = schemaType(row.schema)
  const Icon = type.includes('object') ? Braces : type.includes('array') ? Brackets : CircleDot
  const constraints = schemaConstraints(row.schema, t)
  return <div className="grid min-h-12 grid-cols-[minmax(180px,1.2fr)_120px_minmax(180px,1fr)_minmax(180px,1fr)] items-center gap-3 px-3 py-2 text-xs">
    <div className="flex min-w-0 items-center gap-2" style={{ paddingLeft: `${row.depth * 18}px` }}>
      <Icon className="size-3.5 shrink-0 text-primary" />
      <span className="truncate font-mono font-medium" title={row.name}>{row.name}</span>
      {row.required && <Badge className="shrink-0 px-1.5 py-0.5 text-[9px]" tone="danger">{t('studio.schema.required')}</Badge>}
    </div>
    <div className="flex flex-wrap gap-1">{type.split(' | ').map((item) => <Badge className="px-1.5 py-0.5 text-[9px]" key={item} tone="primary">{item}</Badge>)}</div>
    <span className="leading-5 text-muted-foreground">{row.schema.description ?? row.schema.title ?? '-'}</span>
    <span className="break-words font-mono text-[10px] leading-4 text-muted-foreground">{constraints.length ? constraints.join(' · ') : '-'}</span>
  </div>
}

function flattenSchema(schema: JsonSchema) {
  const rows: SchemaRow[] = []
  const walk = (value: JsonSchema, name: string, depth: number, required: boolean) => {
    rows.push({ depth, name, required, schema: value })
    for (const [childName, child] of Object.entries(value.properties ?? {})) {
      walk(child, childName, depth + 1, value.required?.includes(childName) ?? false)
    }
    if (value.items && (value.items.properties || value.items.items)) walk(value.items, '[]', depth + 1, false)
    for (const key of ['oneOf', 'anyOf', 'allOf'] as const) {
      value[key]?.forEach((variant, index) => walk(variant, `${key}[${index + 1}]`, depth + 1, false))
    }
  }
  const properties = Object.entries(schema.properties ?? {})
  if (properties.length > 0) {
    for (const [name, child] of properties) walk(child, name, 0, schema.required?.includes(name) ?? false)
  } else if (Object.keys(schema).length > 0) {
    walk(schema, '$', 0, false)
  }
  return rows
}

function schemaType(schema: JsonSchema) {
  const type = schema.type ?? (schema.properties ? 'object' : schema.items ? 'array' : schema.enum ? 'enum' : 'any')
  return Array.isArray(type) ? type.join(' | ') : type
}

function schemaConstraints(schema: JsonSchema, t: (key: string, options?: Record<string, unknown>) => string) {
  const values: string[] = []
  if (schema.format) values.push(t('studio.schema.formatValue', { value: schema.format }))
  if (schema.enum) values.push(t('studio.schema.enumValue', { value: schema.enum.map(displayValue).join(', ') }))
  if (schema.default !== undefined) values.push(t('studio.schema.defaultValue', { value: displayValue(schema.default) }))
  if (schema.const !== undefined) values.push(t('studio.schema.constValue', { value: displayValue(schema.const) }))
  if (schema.minimum !== undefined || schema.maximum !== undefined) values.push(t('studio.schema.rangeValue', { min: schema.minimum ?? '-inf', max: schema.maximum ?? '+inf' }))
  if (schema.minLength !== undefined || schema.maxLength !== undefined) values.push(t('studio.schema.lengthValue', { min: schema.minLength ?? 0, max: schema.maxLength ?? '+inf' }))
  if (schema.pattern) values.push(t('studio.schema.patternValue', { value: schema.pattern }))
  return values
}

function displayValue(value: unknown) {
  return typeof value === 'string' ? value : JSON.stringify(value)
}

function isSchema(value: unknown): value is JsonSchema {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value)
}
