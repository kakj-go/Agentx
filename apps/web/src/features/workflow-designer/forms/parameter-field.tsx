import { AlertTriangle, Plus, Trash2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '../../../shared/ui/button'
import { Input } from '../../../shared/ui/input'
import { Select } from '../../../shared/ui/select'
import { Textarea } from '../../../shared/ui/textarea'
import { MarkdownEditor } from '../../../shared/ui/markdown-editor'
import type { JsonSchemaProperty, ResourceOption, UiField } from '../model/types'
import { CodeEditor } from './code-editor'
import { ExpressionEditor } from './expression-editor'

const EMPTY_JSON_OBJECT = {}
const EMPTY_JSON_ARRAY: unknown[] = []

export function ParameterField({ name, schema, ui, value, required, error, parameters, providerOptions = [], workflowId, onChange, onValidityChange }: { name: string; schema: JsonSchemaProperty; ui?: UiField; value: unknown; required?: boolean; error?: string; parameters: Record<string, unknown>; providerOptions?: ResourceOption[]; workflowId?: string; onChange: (value: unknown) => void; onValidityChange?: (valid: boolean) => void }) {
  const label = ui?.label ?? schema.title ?? humanize(name)
  const visible = !ui?.visibleWhen || parameters[ui.visibleWhen.field] === ui.visibleWhen.equals
  const control = ui?.control
  useEffect(() => { onValidityChange?.(control ? SUPPORTED_CONTROLS.has(control) : false) }, [control, onValidityChange])
  if (!visible) return null
  if (!control || !SUPPORTED_CONTROLS.has(control)) return <UnsupportedControl label={label} control={control} />
  const field = (content: React.ReactNode) => <Field error={error} label={label} required={required} testId={`parameter-${name}`}>{content}</Field>
  if (control === 'select') return field(<Select className="w-full" onValueChange={onChange} options={(ui.options ?? schema.enum ?? []).map(selectOption)} value={value === undefined ? '' : String(value)} />)
  if (control === 'provider_options') return field(<Select className="w-full" onValueChange={onChange} options={providerOptions} value={value === undefined ? '' : String(value)} />)
  if (control === 'boolean') return <div><label className="flex items-center gap-2 text-xs"><input checked={Boolean(value)} className="size-4 accent-primary" onChange={(event) => onChange(event.target.checked)} type="checkbox" /><span>{label}</span></label>{error && <p className="mt-1 text-[10px] text-danger">{error}</p>}</div>
  if (control === 'number') return field(<Input max={schema.maximum} min={schema.minimum} onChange={(event) => onChange(event.target.value === '' ? undefined : Number(event.target.value))} type="number" value={value === undefined ? '' : String(value)} />)
  if (control === 'textarea') return field(<Textarea onChange={(event) => onChange(event.target.value)} value={String(value ?? '')} />)
  if (control === 'prompt') return field(<RichPromptControl onChange={onChange} value={String(value ?? '')} />)
  if (control === 'expression') return field(<ExpressionEditor onChange={onChange} value={String(value ?? '')} workflowId={workflowId} />)
  if (control === 'json') return field(<JsonControl fallback={schema.type === 'array' ? EMPTY_JSON_ARRAY : EMPTY_JSON_OBJECT} onChange={onChange} onValidityChange={onValidityChange} value={value} />)
  if (control === 'code') return field(<CodeControl language={codeLanguage(ui.languageField ? parameters[ui.languageField] : undefined)} onChange={onChange} value={String(value ?? '')} />)
  if (control === 'collection') return field(<CollectionControl itemSchema={schema.items} onChange={onChange} value={value} />)
  if (control === 'fixed_collection') return field(<FixedCollectionControl onChange={onChange} value={value} />)
  if (control === 'mapper') return field(<MapperControl onChange={onChange} value={value} />)
  return field(<Input onChange={(event) => onChange(event.target.value)} value={String(value ?? '')} />)
}

function RichPromptControl({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  return <div className="h-[196px] overflow-hidden rounded-md border border-border bg-surface shadow-inner focus-within:border-primary focus-within:ring-2 focus-within:ring-primary/15"><MarkdownEditor onChange={onChange} value={value} /></div>
}

function CodeControl({ language, value, onChange }: { language: string; value: string; onChange: (value: string) => void }) {
  return <div className="overflow-hidden rounded-md border border-border bg-canvas shadow-inner focus-within:border-primary focus-within:ring-2 focus-within:ring-primary/15"><div className="flex h-7 items-center justify-between border-b border-border px-2.5 text-[9px] font-medium uppercase tracking-wide text-muted-foreground"><span>{language}</span><span>Source</span></div><CodeEditor height="238px" language={language} onChange={onChange} value={value} /></div>
}

function JsonControl({ value, fallback, onChange, onValidityChange }: { value: unknown; fallback: unknown; onChange: (value: unknown) => void; onValidityChange?: (valid: boolean) => void }) {
  const { t } = useTranslation()
  const [text, setText] = useState(() => JSON.stringify(value ?? fallback, null, 2))
  const [invalid, setInvalid] = useState(false)
  useEffect(() => setText(JSON.stringify(value ?? fallback, null, 2)), [fallback, value])
  return <div><CodeEditor language="json" onChange={(next) => { setText(next); try { onChange(JSON.parse(next)); setInvalid(false); onValidityChange?.(true) } catch { setInvalid(true); onValidityChange?.(false) } }} value={text} />{invalid && <p className="mt-1 text-[10px] text-danger">{t('studio.invalidJson')}</p>}</div>
}

function CollectionControl({ value, itemSchema, onChange }: { value: unknown; itemSchema?: JsonSchemaProperty; onChange: (value: unknown[]) => void }) {
  const { t } = useTranslation()
  const items = Array.isArray(value) ? value : []
  const update = (index: number, next: unknown) => onChange(items.map((item, current) => current === index ? next : item))
  return <div className="space-y-2">{items.map((item, index) => <div className="flex items-start gap-2" key={index}>{itemSchema?.type === 'object' ? <Textarea className="min-h-20 font-mono text-[11px]" onChange={(event) => { try { update(index, JSON.parse(event.target.value)) } catch { /* Keep the last valid collection item. */ } }} value={JSON.stringify(item, null, 2)} /> : <Input onChange={(event) => update(index, event.target.value)} value={String(item ?? '')} />}<Button aria-label={t('studio.remove', { index: index + 1 })} onClick={() => onChange(items.filter((_, current) => current !== index))} size="icon" variant="ghost"><Trash2 className="size-3.5" /></Button></div>)}<Button onClick={() => onChange([...items, itemSchema?.type === 'object' ? {} : ''])} size="sm" variant="secondary"><Plus className="size-3.5" />{t('studio.addItem')}</Button></div>
}

function FixedCollectionControl({ value, onChange }: { value: unknown; onChange: (value: Record<string, unknown>) => void }) {
  const { t } = useTranslation()
  const entries = Object.entries(value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {})
  const replace = (index: number, key: string, next: unknown) => onChange(Object.fromEntries(entries.map(([currentKey, currentValue], current) => current === index ? [key, next] : [currentKey, currentValue])))
  return <div className="space-y-2">{entries.map(([key, item], index) => <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_32px] gap-2" key={`${key}-${index}`}><Input aria-label={t('studio.key')} onChange={(event) => replace(index, event.target.value, item)} value={key} /><Input aria-label={t('studio.value')} onChange={(event) => replace(index, key, event.target.value)} value={typeof item === 'string' ? item : JSON.stringify(item)} /><Button aria-label={t('studio.removeField', { key })} onClick={() => onChange(Object.fromEntries(entries.filter((_, current) => current !== index)))} size="icon" variant="ghost"><Trash2 className="size-3.5" /></Button></div>)}<Button onClick={() => onChange({ ...Object.fromEntries(entries), [`field${entries.length + 1}`]: '' })} size="sm" variant="secondary"><Plus className="size-3.5" />{t('studio.addField')}</Button></div>
}

function MapperControl({ value, onChange }: { value: unknown; onChange: (value: Record<string, unknown>) => void }) {
  const { t } = useTranslation()
  const entries = Object.entries(value && typeof value === 'object' && !Array.isArray(value) ? value as Record<string, unknown> : {})
  const replace = (index: number, key: string, next: unknown) => onChange(Object.fromEntries(entries.map(([currentKey, currentValue], current) => current === index ? [key, next] : [currentKey, currentValue])))
  return <div className="space-y-2" data-testid="mapper-control">{entries.map(([key, item], index) => <div className="grid grid-cols-[minmax(0,1fr)_minmax(0,1.35fr)_32px] gap-2" key={`${key}-${index}`}><Input aria-label={t('studio.key')} onChange={(event) => replace(index, event.target.value, item)} value={key} /><Input aria-label={t('studio.value')} onChange={(event) => replace(index, key, event.target.value)} value={typeof item === 'string' ? item : JSON.stringify(item)} /><Button aria-label={t('studio.removeField', { key })} onClick={() => onChange(Object.fromEntries(entries.filter((_, current) => current !== index)))} size="icon" variant="ghost"><Trash2 className="size-3.5" /></Button></div>)}<Button onClick={() => onChange({ ...Object.fromEntries(entries), [`field${entries.length + 1}`]: '' })} size="sm" variant="secondary"><Plus className="size-3.5" />{t('studio.addField')}</Button></div>
}

function UnsupportedControl({ label, control }: { label: string; control?: string }) { const { t } = useTranslation(); return <div className="border border-danger/40 bg-danger/5 p-3 text-xs text-danger"><div className="flex items-center gap-2 font-medium"><AlertTriangle className="size-3.5" />{label}</div><p className="mt-1 text-[10px]">{t('studio.unsupported', { control: control ?? 'missing' })}</p></div> }
function Field({ children, label, required, error, testId }: { children: React.ReactNode; label: string; required?: boolean; error?: string; testId?: string }) { return <label className="block text-xs" data-testid={testId}><span className="mb-1.5 block text-muted-foreground">{label}{required && <span className="ml-1 text-danger">*</span>}</span>{children}{error && <span className="mt-1 block text-[10px] text-danger">{error}</span>}</label> }
const humanize = (value: string) => value.replace(/([a-z])([A-Z])/g, '$1 $2').replaceAll('_', ' ').replace(/^./, (letter) => letter.toUpperCase())
const codeLanguage = (runner: unknown) => runner === 'javascript' ? 'javascript' : runner === 'shell' ? 'shell' : runner === 'browser' ? 'typescript' : 'python'
const selectOption = (item: unknown) => item !== null && typeof item === 'object' && 'value' in item && 'label' in item ? { value: String(item.value), label: String(item.label) } : { value: String(item), label: String(item) }
export const SUPPORTED_CONTROLS = new Set(['text', 'textarea', 'number', 'boolean', 'select', 'provider_options', 'collection', 'fixed_collection', 'mapper', 'expression', 'prompt', 'json', 'code'])
