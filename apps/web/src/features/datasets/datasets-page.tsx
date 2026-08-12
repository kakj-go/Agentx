import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { Database, Plus } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { Dataset, PageResponse } from '../../shared/api/types'
import { EntityCell } from '../../shared/components/entity-cell'
import { EntityDeleteButton } from '../../shared/components/entity-delete-button'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge } from '../../shared/components/status-badge'
import { useLocaleFormat } from '../../shared/lib/locale-format'
import { localizedValue } from '../../shared/lib/localized-value'
import { Button } from '../../shared/ui/button'
import { useToast } from '../../shared/ui/toast'

export function DatasetsPage() {
  const { t } = useTranslation()
  const { formatDateTime } = useLocaleFormat()
  const auth = useAuth()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const [open, setOpen] = useState(false)
  const datasets = useQuery({ queryKey: ['datasets'], queryFn: () => apiRequest<PageResponse<Dataset>>('/datasets?pageSize=100') })
  const create = useMutation({ mutationFn: (values: Record<string, string>) => apiRequest<Dataset>('/datasets', { method: 'POST', body: jsonBody({ name: values.name, description: values.description || null, visibility: values.visibility }) }), onSuccess: async (value) => { await queryClient.invalidateQueries({ queryKey: ['datasets'] }); showToast(t('datasets.created')); navigate(`/datasets/${value.id}`) } })
  const columns = useMemo<Array<ColumnDef<Dataset>>>(() => [
    { accessorKey: 'name', header: t('common.name'), cell: ({ row }) => <EntityCell detail={`${t('datasets.revision')} ${row.original.revision}`} icon={Database} name={row.original.name} /> },
    { accessorKey: 'latestVersion', header: t('common.version'), cell: ({ row }) => row.original.latestVersion ? `v${row.original.latestVersion}` : '—' },
    { accessorKey: 'caseCount', header: t('datasets.cases') },
    { accessorKey: 'visibility', header: t('datasets.visibility'), cell: ({ row }) => localizedValue(t, 'datasets', row.original.visibility) },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge label={localizedValue(t, 'common', row.original.status)} status={row.original.status === 'active' ? 'active' : 'inactive'} /> },
    { accessorKey: 'updatedAt', header: t('common.updatedAt'), cell: ({ row }) => formatDateTime(row.original.updatedAt) },
    { id: 'actions', header: '', cell: ({ row }) => <div className="flex gap-1"><Button asChild size="sm" variant="ghost"><Link to={`/datasets/${row.original.id}`}>{t('datasets.manage')}</Link></Button><EntityDeleteButton canDelete={auth.hasPermission('dataset:delete')} deletePath={`/datasets/${row.original.id}`} entityId={row.original.id} entityName={row.original.name} entityType="dataset" onDeleted={async () => { await queryClient.invalidateQueries({ queryKey: ['datasets'] }); showToast(t('datasets.deleted')) }} /></div> },
  ], [auth, formatDateTime, queryClient, showToast, t])
  const fields: EntityFormField[] = [{ name: 'name', label: t('common.name'), required: true }, { name: 'description', label: t('common.description'), type: 'textarea' }, { name: 'visibility', label: t('datasets.visibility'), type: 'select', defaultValue: 'department', options: [{ value: 'private', label: t('datasets.private') }, { value: 'department', label: t('datasets.departmentVisible') }, { value: 'company', label: t('datasets.companyVisible') }] }]
  return <>
    <ListPage action={auth.hasPermission('dataset:manage') ? <Button onClick={() => setOpen(true)}><Plus className="size-4" />{t('datasets.create')}</Button> : undefined} columns={columns} data={datasets.data?.items ?? []} description={t('datasets.description')} getSearchText={(row) => `${row.name} ${row.description ?? ''}`} getStatus={(row) => row.status === 'active' ? 'active' : 'inactive'} searchPlaceholder={t('datasets.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'inactive', label: t('common.inactive') }]} title={t('datasets.title')} />
    {open && <EntityFormDialog cancelLabel={t('common.cancel')} fields={fields} onClose={() => setOpen(false)} onSubmit={(values) => create.mutateAsync(values).then(() => undefined)} open submitLabel={t('common.save')} title={t('datasets.create')} />}
  </>
}
