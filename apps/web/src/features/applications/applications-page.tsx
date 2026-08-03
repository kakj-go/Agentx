import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { AppWindow, Plus } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { Application, PageResponse, Workflow } from '../../shared/api/types'
import { EntityCell } from '../../shared/components/entity-cell'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { ListPage } from '../../shared/components/list-page'
import { PrerequisiteAction } from '../../shared/components/prerequisite-action'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { useToast } from '../../shared/ui/toast'

export function ApplicationsPage() {
  const { t } = useTranslation()
  const auth = useAuth()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const [open, setOpen] = useState(false)
  const applications = useQuery({ queryKey: ['applications'], queryFn: () => apiRequest<PageResponse<Application>>('/applications?pageSize=100') })
  const workflows = useQuery({ queryKey: ['workflows', 'application-options'], queryFn: () => apiRequest<PageResponse<Workflow>>('/workflows?pageSize=100&status=active') })
  const create = useMutation({
    mutationFn: (values: Record<string, string>) => apiRequest<Application>('/applications', { method: 'POST', body: jsonBody({ workflowId: values.workflowId, name: values.name, slug: values.slug, description: values.description || null, visibility: values.visibility }) }),
    onSuccess: async (value) => { await queryClient.invalidateQueries({ queryKey: ['applications'] }); showToast(t('m3.created')); navigate(`/applications/${value.id}`) },
  })
  const columns = useMemo<Array<ColumnDef<Application>>>(() => [
    { accessorKey: 'name', header: t('table.application'), cell: ({ row }) => <EntityCell detail={`${row.original.workflowName} · ${row.original.slug}`} icon={AppWindow} name={row.original.name} /> },
    { accessorKey: 'workflowName', header: t('table.workflow') },
    { accessorKey: 'activeVersionNumber', header: t('pages.applications.deployment'), cell: ({ row }) => row.original.activeVersionNumber ? `v${row.original.activeVersionNumber}` : '—' },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status === 'active' ? 'active' : row.original.status === 'draft' ? 'draft' : 'inactive'} /> },
    { accessorKey: 'updatedAt', header: t('common.updatedAt'), cell: ({ row }) => new Date(row.original.updatedAt).toLocaleString() },
    { id: 'actions', header: '', cell: ({ row }) => <Button asChild size="sm" variant="ghost"><Link to={`/applications/${row.original.id}`}>{t('m3.manage')}</Link></Button> },
  ], [t])
  const fields: EntityFormField[] = [
    { name: 'workflowId', label: t('table.workflow'), type: 'select', required: true, options: (workflows.data?.items ?? []).map((item) => ({ value: item.id, label: item.name })) },
    { name: 'name', label: t('common.name'), required: true },
    { name: 'slug', label: 'Slug', required: true, placeholder: 'customer-service' },
    { name: 'description', label: t('common.description'), type: 'textarea' },
    { name: 'visibility', label: t('m2.visibility'), type: 'select', defaultValue: 'department', options: [{ value: 'private', label: t('m2.private') }, { value: 'department', label: t('m2.departmentVisible') }, { value: 'company', label: t('m2.companyVisible') }] },
  ]
  return <>
    <ListPage action={auth.hasPermission('application:manage') ? <PrerequisiteAction description={t('prerequisites.applicationDescription')} loading={workflows.isLoading} onReady={() => setOpen(true)} requirements={[{ key: 'workflow', label: t('prerequisites.workflow'), met: Boolean(workflows.data?.items.length), href: auth.hasPermission('workflow:create') ? '/workflows' : undefined, actionLabel: t('prerequisites.goWorkflows') }]}><Plus className="size-4" />{t('pages.applications.create')}</PrerequisiteAction> : undefined} columns={columns} data={applications.data?.items ?? []} description={t('pages.applications.description')} getSearchText={(row) => `${row.name} ${row.workflowName} ${row.slug}`} getStatus={(row) => row.status === 'active' ? 'active' : row.status === 'draft' ? 'draft' : 'inactive'} searchPlaceholder={t('pages.applications.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'draft', label: t('common.draft') }, { value: 'inactive', label: t('common.inactive') }]} title={t('pages.applications.title')} />
    {applications.error && <p className="fixed bottom-5 left-1/2 z-20 -translate-x-1/2 rounded-lg bg-danger px-4 py-2 text-xs text-white">{String(applications.error)}</p>}
    {open && <EntityFormDialog cancelLabel={t('common.cancel')} fields={fields} onClose={() => setOpen(false)} onSubmit={(values) => create.mutateAsync(values).then(() => undefined)} open submitLabel={t('common.save')} title={t('pages.applications.create')} />}
  </>
}
