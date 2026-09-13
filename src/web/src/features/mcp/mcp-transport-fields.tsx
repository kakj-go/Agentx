import { Plus, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '../../shared/ui/button'
import { Input } from '../../shared/ui/input'
import { ResourcePicker } from '../workflow-designer/forms/resource-picker'
import type { ResourceOption } from '../workflow-designer/model/types'
import { useMcpDependencyOptions } from './use-mcp-dependency-options'

type EnvironmentReference = { name: string; credentialId: string }

export function McpRuntimeSandboxPicker({ departmentId, value, onChange }: {
  departmentId: string
  value: string
  onChange: (value: string) => void
}) {
  const resources = useMcpDependencyOptions(departmentId, 'sandbox_profile')
  const options = resources.options.map((option) => ({
    ...option,
    detail: [option.detail, option.versionId].filter(Boolean).join(' · '),
  }))
  const selectedResourceId = value.split(':', 1)[0]
  return <div>
    <input name="runtimeSandbox" type="hidden" value={value} />
    <DependencyState departmentId={departmentId} error={resources.error} loading={resources.loading} />
    <ResourcePicker onAuthorize={resources.authorize} onChange={(id, versionId) => onChange(versionId ? `${id}:${versionId}` : '')} onRequest={resources.request} options={options} value={selectedResourceId} />
  </div>
}

export function McpEnvironmentCredentials({ departmentId, value, onChange }: {
  departmentId: string
  value: string
  onChange: (value: string) => void
}) {
  const { t } = useTranslation()
  const references = parseReferences(value)
  const resources = useMcpDependencyOptions(departmentId, 'credential')
  const replace = (index: number, patch: Partial<EnvironmentReference>) => {
    const next = references.map((reference, current) => current === index ? { ...reference, ...patch } : reference)
    onChange(JSON.stringify(next))
  }
  const remove = (index: number) => onChange(JSON.stringify(references.filter((_, current) => current !== index)))
  return <div className="space-y-2">
    <input name="environmentCredentials" type="hidden" value={value} />
    <DependencyState departmentId={departmentId} error={resources.error} loading={resources.loading} />
    {references.map((reference, index) => <div className="grid grid-cols-[minmax(120px,0.7fr)_minmax(180px,1.3fr)_auto] items-center gap-2" key={`${index}:${reference.name}`}>
      <Input aria-label={t('mcp.environmentVariableName')} onChange={(event) => replace(index, { name: event.target.value.toUpperCase() })} placeholder="API_TOKEN" value={reference.name} />
      <ResourcePicker onAuthorize={resources.authorize} onChange={(credentialId) => replace(index, { credentialId })} onRequest={resources.request} options={resources.options} value={reference.credentialId} />
      <Button aria-label={t('mcp.removeEnvironmentCredential')} onClick={() => remove(index)} size="sm" type="button" variant="ghost"><Trash2 className="size-3.5" /></Button>
    </div>)}
    <Button onClick={() => onChange(JSON.stringify([...references, { name: '', credentialId: '' }]))} size="sm" type="button" variant="secondary"><Plus className="size-3.5" />{t('mcp.addEnvironmentCredential')}</Button>
  </div>
}

export function McpCredentialPicker({ departmentId, value, onChange }: {
  departmentId: string
  value: string
  onChange: (value: string) => void
}) {
  const { t } = useTranslation()
  const resources = useMcpDependencyOptions(departmentId, 'credential')
  const options: ResourceOption[] = [{ value: '', label: t('mcp.noCredential'), accessState: 'authorized' }, ...resources.options]
  return <div>
    <input name="credential" type="hidden" value={value} />
    <DependencyState departmentId={departmentId} error={resources.error} loading={resources.loading} />
    <ResourcePicker ariaLabel={t('mcp.credential')} onAuthorize={resources.authorize} onChange={onChange} onRequest={resources.request} options={options} value={value} />
  </div>
}

function DependencyState({ departmentId, loading, error }: { departmentId: string; loading: boolean; error: unknown }) {
  const { t } = useTranslation()
  if (!departmentId) return <p className="mb-2 text-[11px] text-muted-foreground">{t('mcp.selectOwnerDepartmentFirst')}</p>
  if (loading) return <p className="mb-2 text-[11px] text-muted-foreground">{t('common.loading')}</p>
  if (error) return <p className="mb-2 text-[11px] text-danger">{error instanceof Error ? error.message : t('mcp.dependencyOptionsFailed')}</p>
  return null
}

function parseReferences(value: string): EnvironmentReference[] {
  try {
    const parsed = JSON.parse(value || '[]') as unknown
    if (!Array.isArray(parsed)) return []
    return parsed.filter((item): item is EnvironmentReference => Boolean(item)
      && typeof (item as EnvironmentReference).name === 'string'
      && typeof (item as EnvironmentReference).credentialId === 'string')
  } catch {
    return []
  }
}
