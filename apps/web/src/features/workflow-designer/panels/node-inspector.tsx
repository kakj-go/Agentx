import { Trash2 } from 'lucide-react'
import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '../../../shared/ui/button'
import { Input } from '../../../shared/ui/input'
import { Select } from '../../../shared/ui/select'
import { useProviderOptions } from '../api/use-provider-options'
import type { NodeManifest, ResourceOption, ResourceType, StudioNodeData, UiField } from '../model/types'
import { ParameterField, SUPPORTED_CONTROLS } from '../forms/parameter-field'
import { NodeIcon } from '../nodes/node-icon'

export function NodeInspector({ data, manifest, resources, workflowId, onChange, onDelete, fieldErrors = {}, onValidityChange }: { data?: StudioNodeData; manifest?: NodeManifest; resources: Partial<Record<ResourceType, ResourceOption[]>>; workflowId?: string; onChange: (data: Partial<StudioNodeData>) => void; onDelete: () => void; fieldErrors?: Record<string, string>; onValidityChange?: (valid: boolean) => void }) {
  const { t } = useTranslation()
  const unsupported = parameterEntries(manifest).some(([name]) => !SUPPORTED_CONTROLS.has(uiFields(manifest)[name]?.control ?? ''))
  const providerOptions = useProviderOptions(manifest)
  useEffect(() => onValidityChange?.(!unsupported), [onValidityChange, unsupported])
  return <aside className="w-[360px] shrink-0 overflow-y-auto border-l border-border bg-surface"><div className="sticky top-0 z-10 border-b border-border bg-surface/95 px-4 py-3 backdrop-blur"><h2 className="text-xs font-semibold">{t('studio.inspector.title')}</h2>{data?.editorKind === 'action' && <div className="mt-3 flex items-center gap-2.5"><span className="grid size-8 place-items-center rounded-lg bg-primary/10 text-primary"><NodeIcon className="size-4" iconKey={manifest?.iconKey ?? 'box'} /></span><span className="min-w-0"><strong className="block truncate text-xs">{data.label}</strong><span className="block truncate text-[10px] text-muted-foreground">{manifest?.description || manifest?.displayName || data.nodeType}</span></span></div>}</div><div className="p-4">{!data && <p className="mt-1 text-xs text-muted-foreground">{t('studio.inspector.noSelection')}</p>}
    {data?.editorKind === 'action' && <div className="space-y-4"><Field label={t('studio.inspector.name')}><Input onChange={(event) => onChange({ label: event.target.value })} value={data.label} /></Field><label className="flex items-center gap-2 text-xs"><input checked={data.disabled} className="size-4 accent-primary" onChange={(event) => onChange({ disabled: event.target.checked })} type="checkbox" />{t('studio.inspector.disabled')}</label>
      {parameterEntries(manifest).map(([name, schema]) => <ParameterField key={name} error={fieldErrors[`parameters.${name}`]} name={name} onChange={(value) => onChange({ parameters: { ...data.parameters, [name]: value } })} parameters={data.parameters} providerOptions={providerOptions[name]} required={manifest?.parameterSchema.required?.includes(name)} schema={schema} ui={uiFields(manifest)[name]} value={data.parameters[name] ?? schema.default} workflowId={workflowId} />)}
      {resourceSelectors(manifest).map((selector) => <ResourceSelect key={`${selector.resourceType}-${selector.operation}`} label={selector.label ?? selector.resourceType.replaceAll('_', ' ')} onChange={(resourceId, versionId) => onChange({ resourceReferences: [...data.resourceReferences.filter((reference) => reference.bindingId || reference.resourceType !== selector.resourceType), { resourceType: selector.resourceType, resourceId, resourceVersionId: versionId, operation: selector.operation }] })} options={resources[selector.resourceType] ?? []} required={selector.required} optionsMissing={Boolean(selector.required && !(resources[selector.resourceType] ?? []).length)} testId={`resource-selector-${selector.resourceType}`} value={data.resourceReferences.find((reference) => !reference.bindingId && reference.resourceType === selector.resourceType)?.resourceId} />)}
      {manifest?.bindingSlots.map((slot) => <div className="flex items-center justify-between border-t border-border pt-3 text-[11px]" key={slot.name}><span>{slot.name}</span><span className={slot.required ? 'text-warning' : 'text-muted-foreground'}>{t(slot.required ? 'studio.inspector.required' : slot.multiple ? 'studio.inspector.multiple' : 'studio.inspector.optional')}</span></div>)}
    </div>}
    {data?.editorKind === 'binding' && <div className="mt-4 space-y-4"><Field label={t('studio.inspector.attachment')}><Input disabled value={t(`resourceTypes.${data.resourceType}`)} /></Field><ResourceSelect label={t('studio.inspector.resource')} onChange={(resourceId, versionId, resourceName) => onChange({ resourceId, resourceVersionId: versionId, resourceName })} options={resources[data.resourceType] ?? []} testId="attachment-resource" value={data.resourceId} /></div>}
    {data && <Button className="mt-6 w-full" onClick={onDelete} variant="secondary"><Trash2 className="size-4" />{t('studio.inspector.delete')}</Button>}
    </div></aside>
}

function ResourceSelect({ label, options, value, required, optionsMissing, testId, onChange }: { label: string; options: ResourceOption[]; value?: string; required?: boolean; optionsMissing?: boolean; testId?: string; onChange: (id: string, versionId?: string | null, label?: string) => void }) { const { t } = useTranslation(); return <Field label={label} required={required} testId={testId}><Select className="w-full" onValueChange={(id) => { const selected = options.find((item) => item.value === id); onChange(id, selected?.versionId, selected?.label) }} options={options} value={value ?? ''} />{optionsMissing && <span className="mt-1 block text-[10px] text-warning">{t('studio.inspector.noResource')}</span>}</Field> }
function Field({ children, label, required, testId }: { children: React.ReactNode; label: string; required?: boolean; testId?: string }) { return <label className="block text-xs" data-testid={testId}><span className="mb-1.5 block text-muted-foreground">{label}{required && <span className="ml-1 text-danger">*</span>}</span>{children}</label> }
type ResourceSelector = { resourceType: ResourceType; operation: 'view' | 'use' | 'read' | 'write' | 'manage'; required?: boolean; label?: string }
function resourceSelectors(manifest?: NodeManifest): ResourceSelector[] {
  const value = manifest?.uiSchema.resourceSelectors
  if (!Array.isArray(value)) return []
  return value.filter((item): item is ResourceSelector => Boolean(item && typeof item === 'object' && typeof (item as ResourceSelector).resourceType === 'string' && typeof (item as ResourceSelector).operation === 'string'))
}
function parameterEntries(manifest?: NodeManifest) { return Object.entries(manifest?.parameterSchema.properties ?? {}) }
function uiFields(manifest?: NodeManifest): Record<string, UiField> { return manifest?.uiSchema.fields ?? {} }
