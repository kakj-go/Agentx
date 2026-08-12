import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { Box, Plus } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { Department, PageResponse, SandboxProfile } from '../../shared/api/types'
import { EntityCell } from '../../shared/components/entity-cell'
import { EntityDeleteButton } from '../../shared/components/entity-delete-button'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge } from '../../shared/components/status-badge'
import { useLocaleFormat } from '../../shared/lib/locale-format'
import { Button } from '../../shared/ui/button'
import { useToast } from '../../shared/ui/toast'
import { bytesToGb, sandboxVersionBody, sandboxVersionFields } from './sandbox-profile-form'

export function SandboxProfilesPage() {
  const { t } = useTranslation()
  const { formatDateTime } = useLocaleFormat()
  const auth = useAuth()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const [open, setOpen] = useState(false)
  const profiles = useQuery({ queryKey: ['sandbox-profiles'], queryFn: () => apiRequest<PageResponse<SandboxProfile>>('/sandbox-profiles?pageSize=100') })
  const departments = useQuery({ queryKey: ['departments'], queryFn: () => apiRequest<Department[]>('/departments') })
  const create = useMutation({
    mutationFn: (values: Record<string, string>) => apiRequest<SandboxProfile>('/sandbox-profiles', {
      method: 'POST',
      body: jsonBody({
        name: values.name.trim(),
        description: values.description.trim() || null,
        ownerDepartmentId: values.department,
        ...sandboxVersionBody(values, t('sandbox.invalidJson'), t('sandbox.invalidImageTag')),
      }),
    }),
    onSuccess: async () => { await queryClient.invalidateQueries({ queryKey: ['sandbox-profiles'] }); showToast(t('sandbox.created')) },
  })
  const columns = useMemo<Array<ColumnDef<SandboxProfile>>>(() => [
    { accessorKey: 'name', header: t('common.name'), cell: ({ row }) => <EntityCell detail={`${row.original.current.runner} · v${row.original.currentVersionNumber}`} icon={Box} name={row.original.name} /> },
    { id: 'image', header: t('sandbox.image'), cell: ({ row }) => <span className="block max-w-72 truncate font-mono text-[11px]" title={row.original.current.imageDigest}>{row.original.current.imageDigest}</span> },
    { id: 'limits', header: t('sandbox.limits'), cell: ({ row }) => `${formatCpu(row.original.current.cpuMillis)} cores · ${formatBytes(row.original.current.memoryBytes)}` },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status === 'active' ? 'active' : 'inactive'} /> },
    { accessorKey: 'updatedAt', header: t('common.updatedAt'), cell: ({ row }) => formatDateTime(row.original.updatedAt) },
    { id: 'actions', header: '', cell: ({ row }) => <div className="flex gap-1"><Button asChild size="sm" variant="ghost"><Link to={`/sandbox-profiles/${row.original.id}`}>{t('sandbox.details')}</Link></Button><EntityDeleteButton canDelete={auth.hasPermission('sandbox:delete')} deletePath={`/sandbox-profiles/${row.original.id}`} entityId={row.original.id} entityName={row.original.name} entityType="sandbox_profile" onDeleted={async () => { await queryClient.invalidateQueries({ queryKey: ['sandbox-profiles'] }); showToast(t('sandbox.deleted')) }} /></div> },
  ], [auth, formatDateTime, queryClient, showToast, t])
  const fields: EntityFormField[] = [
    { name: 'name', label: t('common.name'), required: true },
    { name: 'description', label: t('common.description'), type: 'textarea' },
    { name: 'department', label: t('sandbox.department'), type: 'select', required: true, options: (departments.data ?? []).map((item) => ({ value: item.id, label: item.name })) },
    ...sandboxVersionFields(t),
  ]
  return <>
    <ListPage action={auth.hasPermission('sandbox:manage') ? <Button onClick={() => setOpen(true)}><Plus className="size-4" />{t('sandbox.create')}</Button> : undefined} columns={columns} data={profiles.data?.items ?? []} description={t('sandbox.description')} getSearchText={(row) => `${row.name} ${row.current.runner} ${row.current.imageDigest}`} getStatus={(row) => row.status} searchPlaceholder={t('sandbox.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'disabled', label: t('common.inactive') }]} title={t('sandbox.title')} />
    {open && <EntityFormDialog cancelLabel={t('common.cancel')} fields={fields} onClose={() => setOpen(false)} onSubmit={(values) => create.mutateAsync(values).then(() => undefined)} open submitLabel={t('common.save')} title={t('sandbox.create')} />}
  </>
}

function formatBytes(value: number) {
  return `${bytesToGb(value)} GB`
}

function formatCpu(value: number) {
  return (value / 1000).toFixed(3).replace(/\.?(0+)$/, '')
}
