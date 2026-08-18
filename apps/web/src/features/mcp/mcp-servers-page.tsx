import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { Plus, ServerCog } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { Credential, Department, McpServer, PageResponse } from '../../shared/api/types'
import { EntityCell } from '../../shared/components/entity-cell'
import { EntityDeleteButton } from '../../shared/components/entity-delete-button'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge } from '../../shared/components/status-badge'
import { useLocaleFormat } from '../../shared/lib/locale-format'
import { Button } from '../../shared/ui/button'
import { useToast } from '../../shared/ui/toast'

export function McpServersPage() {
  const { t } = useTranslation()
  const { formatDateTime } = useLocaleFormat()
  const auth = useAuth()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const [open, setOpen] = useState(false)
  const servers = useQuery({ queryKey: ['mcp-servers'], queryFn: () => apiRequest<PageResponse<McpServer>>('/mcp/servers?pageSize=100') })
  const departments = useQuery({ queryKey: ['departments'], queryFn: () => apiRequest<Department[]>('/departments') })
  const credentials = useQuery({ queryKey: ['credentials', 'mcp-options'], queryFn: () => apiRequest<PageResponse<Credential>>('/credentials?pageSize=100&status=active') })
  const create = useMutation({
    mutationFn: async (values: Record<string, string>) => {
      let configuration: unknown
      try { configuration = JSON.parse(values.configuration || '{}') as unknown } catch { throw new Error(t('mcp.invalidJson')) }
      return apiRequest<McpServer>('/mcp/servers', { method: 'POST', body: jsonBody({ name: values.name, description: values.description || null, transport: values.transport, endpoint: values.endpoint, credentialId: values.credential || null, ownerDepartmentId: values.department, configuration }) })
    },
    onSuccess: async () => { await queryClient.invalidateQueries({ queryKey: ['mcp-servers'] }); showToast(t('mcp.created')) },
  })
  const columns = useMemo<Array<ColumnDef<McpServer>>>(() => [
    { accessorKey: 'name', header: t('common.name'), cell: ({ row }) => <EntityCell detail={row.original.endpoint} icon={ServerCog} name={row.original.name} /> },
    { accessorKey: 'transport', header: t('mcp.transport') },
    { accessorKey: 'toolCount', header: t('mcp.discoveredTools'), cell: ({ row }) => row.original.toolCount },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status === 'active' ? 'active' : 'inactive'} /> },
    { accessorKey: 'updatedAt', header: t('common.updatedAt'), cell: ({ row }) => formatDateTime(row.original.updatedAt) },
    { id: 'actions', header: '', cell: ({ row }) => <div className="flex gap-1"><Button asChild size="sm" variant="ghost"><Link to={`/mcp/${row.original.id}`}>{t('mcp.details')}</Link></Button><EntityDeleteButton canDelete={auth.hasPermission('mcp:delete')} deletePath={`/mcp/servers/${row.original.id}`} entityId={row.original.id} entityName={row.original.name} entityType="mcp_server" onDeleted={async () => { await queryClient.invalidateQueries({ queryKey: ['mcp-servers'] }); showToast(t('mcp.deleted')) }} /></div> },
  ], [auth, formatDateTime, queryClient, showToast, t])
  const fields: EntityFormField[] = [
    { name: 'name', label: t('common.name'), required: true },
    { name: 'description', label: t('common.description') },
    { name: 'transport', label: t('mcp.transport'), type: 'select', defaultValue: 'streamable_http', required: true, options: [{ value: 'streamable_http', label: t('mcp.streamableHttp') }, { value: 'sse', label: t('mcp.legacySse') }] },
    { name: 'endpoint', label: t('mcp.endpoint'), defaultValue: 'http://echo-mcp:8090/mcp', required: true },
    { name: 'credential', label: t('mcp.credential'), type: 'select', options: [{ value: '', label: t('mcp.noCredential') }, ...(credentials.data?.items ?? []).map((item) => ({ value: item.id, label: item.name }))] },
    { name: 'department', label: t('mcp.department'), type: 'select', required: true, options: (departments.data ?? []).map((item) => ({ value: item.id, label: item.name })) },
    { name: 'configuration', label: t('mcp.configuration'), type: 'textarea', defaultValue: '{}' },
  ]
  return <>
    <ListPage action={auth.hasPermission('mcp:manage') ? <Button onClick={() => setOpen(true)}><Plus className="size-4" />{t('mcp.connectMcp')}</Button> : undefined} columns={columns} data={servers.data?.items ?? []} description={t('mcp.description')} getSearchText={(row) => `${row.name} ${row.endpoint} ${row.transport}`} getStatus={(row) => row.status === 'active' ? 'active' : 'inactive'} searchPlaceholder={t('mcp.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'inactive', label: t('common.inactive') }]} title={t('mcp.title')} />
    {open && <EntityFormDialog cancelLabel={t('common.cancel')} fields={fields} onClose={() => setOpen(false)} onSubmit={(values) => create.mutateAsync(values).then(() => undefined)} open submitLabel={t('common.save')} title={t('mcp.connectMcp')} />}
  </>
}
