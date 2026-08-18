import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { CloudCog, Pencil, Plus } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { WorkflowEnvironment } from '../../shared/api/types'
import { EntityDeleteButton } from '../../shared/components/entity-delete-button'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { Tooltip } from '../../shared/ui/tooltip'
import { useToast } from '../../shared/ui/toast'

export function EnvironmentsPage() {
  const { t } = useTranslation()
  const auth = useAuth()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const [editing, setEditing] = useState<WorkflowEnvironment | 'create'>()
  const query = useQuery({ queryKey: ['environments'], queryFn: () => apiRequest<WorkflowEnvironment[]>('/environments') })
  const save = useMutation({
    mutationFn: ({ target, values }: { target: WorkflowEnvironment | 'create'; values: Record<string, string> }) => target === 'create'
      ? apiRequest<WorkflowEnvironment>('/environments', { method: 'POST', body: jsonBody({ code: values.code, name: values.name }) })
      : apiRequest<WorkflowEnvironment>(`/environments/${target.id}`, { method: 'PATCH', body: jsonBody({ name: values.name, status: values.status, version: target.version }) }),
    onSuccess: async () => { await queryClient.invalidateQueries({ queryKey: ['environments'] }); showToast(t('workflows.environmentSaved')) },
  })
  const columns = useMemo<Array<ColumnDef<WorkflowEnvironment>>>(() => [
    { accessorKey: 'name', header: t('workflows.environment'), cell: ({ row }) => <EntityCell detail={row.original.code} icon={CloudCog} name={row.original.name} /> },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status === 'active' ? 'active' : 'inactive'} /> },
    { accessorKey: 'isBuiltin', header: t('workflows.environmentType'), cell: ({ row }) => row.original.isBuiltin ? t('workflows.builtinEnvironment') : t('workflows.customEnvironment') },
    { accessorKey: 'version', header: t('common.version') },
    { id: 'actions', header: '', cell: ({ row }) => <div className="flex gap-1">
      {auth.hasPermission('workflow:publish') && <Tooltip content={t('common.edit')}><Button aria-label={t('common.edit')} onClick={() => setEditing(row.original)} size="icon" variant="ghost"><Pencil className="size-4" /></Button></Tooltip>}
      <EntityDeleteButton canDelete={auth.hasPermission('workflow:delete')} deletePath={`/environments/${row.original.id}`} entityId={row.original.id} entityName={row.original.name} entityType="environment" immutableReason={row.original.isBuiltin ? t('common.deletion.immutable') : undefined} onDeleted={async () => { await queryClient.invalidateQueries({ queryKey: ['environments'] }); showToast(t('workflows.environmentDeleted')) }} />
    </div> },
  ], [auth, queryClient, showToast, t])
  const fields: EntityFormField[] = editing === 'create'
    ? [{ name: 'code', label: t('workflows.environmentCode'), required: true, placeholder: 'staging' }, { name: 'name', label: t('workflows.environmentName'), required: true }]
    : [{ name: 'name', label: t('workflows.environmentName'), defaultValue: editing?.name, required: true }, { name: 'status', label: t('common.status'), type: 'select', defaultValue: editing?.status ?? 'active', required: true, options: [{ value: 'active', label: t('common.active') }, { value: 'disabled', label: t('common.inactive') }] }]

  return <>
    <ListPage action={auth.hasPermission('workflow:publish') ? <Button onClick={() => setEditing('create')}><Plus className="size-4" />{t('workflows.createEnvironment')}</Button> : undefined} columns={columns} data={query.data ?? []} description={t('workflows.environmentDescription')} getSearchText={(row) => `${row.name} ${row.code}`} getStatus={(row) => row.status === 'active' ? 'active' : 'inactive'} searchPlaceholder={t('workflows.searchEnvironments')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'inactive', label: t('common.inactive') }]} title={t('workflows.environmentManagement')} />
    {query.error && <p className="fixed bottom-5 left-1/2 z-20 -translate-x-1/2 rounded-lg bg-danger px-4 py-2 text-xs text-white">{String(query.error)}</p>}
    {editing && <EntityFormDialog cancelLabel={t('common.cancel')} fields={fields} onClose={() => setEditing(undefined)} onSubmit={(values) => save.mutateAsync({ target: editing, values }).then(() => undefined)} open submitLabel={t('common.save')} title={editing === 'create' ? t('workflows.createEnvironment') : t('workflows.editEnvironment')} />}
  </>
}
