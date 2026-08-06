import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { Plus, Sparkles } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { Department, PageResponse, Skill } from '../../shared/api/types'
import { EntityCell } from '../../shared/components/entity-cell'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { useToast } from '../../shared/ui/toast'

export function SkillsPage() {
  const { t } = useTranslation()
  const auth = useAuth()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const [open, setOpen] = useState(false)
  const skills = useQuery({ queryKey: ['skills'], queryFn: () => apiRequest<PageResponse<Skill>>('/skills?pageSize=100') })
  const departments = useQuery({ queryKey: ['departments'], queryFn: () => apiRequest<Department[]>('/departments') })
  const create = useMutation({ mutationFn: (values: Record<string, string>) => apiRequest<Skill>('/skills', { method: 'POST', body: jsonBody({ name: values.name, alias: values.alias, description: values.description, ownerDepartmentId: values.department }) }), onSuccess: async () => { await queryClient.invalidateQueries({ queryKey: ['skills'] }); showToast(t('m2.created')) } })
  const columns = useMemo<Array<ColumnDef<Skill>>>(() => [
    { accessorKey: 'name', header: t('table.skill'), cell: ({ row }) => <EntityCell detail={`${row.original.alias} · ${t('m2.workspaceRevision', { revision: row.original.draftRevision })}`} icon={Sparkles} name={row.original.name} /> },
    { accessorKey: 'latestVersion', header: t('pages.skills.version'), cell: ({ row }) => row.original.latestVersion ? `v${row.original.latestVersion}` : '—' },
    { accessorKey: 'draftRevision', header: t('m2.workspaceRevisionShort'), cell: ({ row }) => `r${row.original.draftRevision}` },
    { accessorKey: 'grantCount', header: t('pages.skills.workflows') },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status === 'active' ? 'active' : row.original.status === 'draft' ? 'draft' : 'inactive'} /> },
    { id: 'actions', header: '', cell: ({ row }) => <Button asChild size="sm" variant="ghost"><Link to={`/skills/${row.original.id}`}>{t('m2.details')}</Link></Button> },
  ], [t])
  const fields: EntityFormField[] = [{ name: 'name', label: t('common.name'), required: true }, { name: 'alias', label: t('m2.alias'), required: true }, { name: 'description', label: t('m2.description'), type: 'textarea', required: true }, { name: 'department', label: t('m2.department'), type: 'select', required: true, options: (departments.data ?? []).map((item) => ({ value: item.id, label: item.name })) }]
  return <>
    <ListPage action={auth.hasPermission('skill:manage') ? <Button onClick={() => setOpen(true)}><Plus className="size-4" />{t('m2.createSkill')}</Button> : undefined} columns={columns} data={skills.data?.items ?? []} description={t('pages.skills.description')} getSearchText={(row) => `${row.name} ${row.alias} ${row.description ?? ''}`} getStatus={(row) => row.status === 'active' ? 'active' : row.status === 'draft' ? 'draft' : 'inactive'} searchPlaceholder={t('pages.skills.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'draft', label: t('common.draft') }, { value: 'inactive', label: t('common.inactive') }]} title={t('pages.skills.title')} />
    {open && <EntityFormDialog cancelLabel={t('common.cancel')} fields={fields} onClose={() => setOpen(false)} onSubmit={(values) => create.mutateAsync(values).then(() => undefined)} open submitLabel={t('common.save')} title={t('m2.createSkill')} />}
  </>
}
