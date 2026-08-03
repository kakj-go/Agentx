import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { KeyRound, Plus } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { Credential, Department, PageResponse } from '../../shared/api/types'
import { EntityCell } from '../../shared/components/entity-cell'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { useToast } from '../../shared/ui/toast'

export function CredentialsPage() {
  const { t } = useTranslation()
  const auth = useAuth()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const [open, setOpen] = useState(false)
  const credentials = useQuery({ queryKey: ['credentials'], queryFn: () => apiRequest<PageResponse<Credential>>('/credentials?pageSize=100') })
  const departments = useQuery({ queryKey: ['departments'], queryFn: () => apiRequest<Department[]>('/departments') })
  const create = useMutation({
    mutationFn: (values: Record<string, string>) => apiRequest<Credential>('/credentials', { method: 'POST', body: jsonBody({ name: values.name, credentialType: values.type, secret: parseSecret(values.type, values.secret, t('m2.invalidJson')), ownerDepartmentId: values.department }) }),
    onSuccess: async () => { await queryClient.invalidateQueries({ queryKey: ['credentials'] }); showToast(t('m2.created')) },
  })
  const columns = useMemo<Array<ColumnDef<Credential>>>(() => [
    { accessorKey: 'name', header: t('common.name'), cell: ({ row }) => <EntityCell detail={`${row.original.credentialType} · ${row.original.maskedHint}`} icon={KeyRound} name={row.original.name} /> },
    { accessorKey: 'storageMode', header: t('m2.storageMode') },
    { accessorKey: 'currentSecretVersion', header: t('common.version'), cell: ({ row }) => `v${row.original.currentSecretVersion}` },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status === 'active' ? 'active' : 'inactive'} /> },
    { accessorKey: 'updatedAt', header: t('common.updatedAt'), cell: ({ row }) => new Date(row.original.updatedAt).toLocaleString() },
    { id: 'actions', header: '', cell: ({ row }) => <Button asChild size="sm" variant="ghost"><Link to={`/credentials/${row.original.id}`}>{t('m2.details')}</Link></Button> },
  ], [t])
  const fields: EntityFormField[] = [
    { name: 'name', label: t('common.name'), required: true },
    { name: 'type', label: t('m2.credentialType'), type: 'select', defaultValue: 'api_key', options: [{ value: 'api_key', label: t('m2.apiKey') }, { value: 'bearer', label: t('m2.bearer') }, { value: 'basic', label: t('m2.basic') }, { value: 'custom_json', label: t('m2.customJson') }] },
    { name: 'secret', label: t('m2.secret'), type: 'password', required: true, placeholder: t('m2.passwordMask') },
    { name: 'department', label: t('m2.department'), type: 'select', required: true, options: (departments.data ?? []).map((item) => ({ value: item.id, label: item.name })) },
  ]
  return <>
    <ListPage action={auth.hasPermission('credential:manage') ? <Button onClick={() => setOpen(true)}><Plus className="size-4" />{t('m2.createCredential')}</Button> : undefined} columns={columns} data={credentials.data?.items ?? []} description={t('m2.credentialDescription')} getSearchText={(row) => `${row.name} ${row.credentialType} ${row.maskedHint}`} getStatus={(row) => row.status === 'active' ? 'active' : 'inactive'} searchPlaceholder={t('m2.searchCredentials')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'inactive', label: t('common.inactive') }]} title={t('nav.credentials')} />
    {open && <EntityFormDialog cancelLabel={t('common.cancel')} fields={fields} onClose={() => setOpen(false)} onSubmit={(values) => create.mutateAsync(values).then(() => undefined)} open submitLabel={t('common.save')} title={t('m2.createCredential')} />}
  </>
}

function parseSecret(type: string, value: string, invalidMessage: string): unknown {
  if (type === 'basic' || type === 'custom_json') {
    try { return JSON.parse(value) as unknown } catch { throw new Error(invalidMessage) }
  }
  return value
}
