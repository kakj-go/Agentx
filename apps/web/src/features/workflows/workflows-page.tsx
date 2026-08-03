import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { Blocks, Plus } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { PageResponse, Workflow } from '../../shared/api/types'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { useToast } from '../../shared/ui/toast'

export function WorkflowsPage() {
  const { t } = useTranslation()
  const auth = useAuth()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const [open, setOpen] = useState(false)
  const query = useQuery({ queryKey: ['workflows'], queryFn: () => apiRequest<PageResponse<Workflow>>('/workflows?pageSize=100') })
  const create = useMutation({
    mutationFn: (values: Record<string, string>) => apiRequest<Workflow>('/workflows', { method: 'POST', body: jsonBody({ name: values.name, description: values.description || null, visibility: values.visibility }) }),
    onSuccess: async (workflow) => { await queryClient.invalidateQueries({ queryKey: ['workflows'] }); showToast(t('m2.created')); navigate(`/workflows/${workflow.id}`) },
  })
  const columns = useMemo<Array<ColumnDef<Workflow>>>(() => [
    { accessorKey: 'name', header: t('table.workflow'), cell: ({ row }) => <EntityCell detail={row.original.description ?? t('m2.draftLabel')} icon={Blocks} name={row.original.name} /> },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status === 'active' ? 'active' : 'inactive'} /> },
    { accessorKey: 'draftRevision', header: 'Revision' },
    { accessorKey: 'latestVersion', header: t('pages.workflows.latestVersion'), cell: ({ row }) => row.original.latestVersion ? `v${row.original.latestVersion}` : '—' },
    { accessorKey: 'ownerName', header: t('common.owner') },
    { accessorKey: 'updatedAt', header: t('common.updatedAt'), cell: ({ row }) => new Date(row.original.updatedAt).toLocaleString() },
    { id: 'actions', header: '', cell: ({ row }) => <div className="flex gap-1"><Button asChild size="sm" variant="ghost"><Link to={`/workflows/${row.original.id}`}>{t('m2.details')}</Link></Button><Button asChild size="sm" variant="ghost"><Link to={`/workflows/${row.original.id}/editor`}>{t('m2.openEditor')}</Link></Button></div> },
  ], [t])
  const fields: EntityFormField[] = [
    { name: 'name', label: t('m2.workflowName'), required: true },
    { name: 'description', label: t('m2.description'), type: 'textarea' },
    { name: 'visibility', label: t('m2.visibility'), type: 'select', defaultValue: 'private', options: [{ value: 'private', label: t('m2.private') }, { value: 'department', label: t('m2.departmentVisible') }, { value: 'company', label: t('m2.companyVisible') }] },
  ]
  return <>
    <ListPage action={auth.hasPermission('workflow:create') ? <Button onClick={() => setOpen(true)}><Plus className="size-4" />{t('m2.createWorkflow')}</Button> : undefined} columns={columns} data={query.data?.items ?? []} description={t('pages.workflows.description')} getSearchText={(row) => `${row.name} ${row.ownerName}`} getStatus={(row) => row.status === 'active' ? 'active' : 'inactive'} searchPlaceholder={t('pages.workflows.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'inactive', label: t('common.inactive') }]} title={t('pages.workflows.title')} />
    {query.error && <p className="fixed bottom-5 left-1/2 z-20 -translate-x-1/2 rounded-lg bg-danger px-4 py-2 text-xs text-white">{String(query.error)}</p>}
    {open && <EntityFormDialog cancelLabel={t('common.cancel')} fields={fields} onClose={() => setOpen(false)} onSubmit={(values) => create.mutateAsync(values).then(() => undefined)} open submitLabel={t('common.save')} title={t('m2.createWorkflow')} />}
  </>
}
