import { useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { KeyRound, Plus } from 'lucide-react'
import { useMemo, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { PageResponse, Permission, Role } from '../../shared/api/types'
import { DataTable } from '../../shared/components/data-table'
import { EntityCell } from '../../shared/components/entity-cell'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { StatusBadge } from '../../shared/components/status-badge'
import { dataScopeLabel, permissionLabel, roleLabel } from '../../shared/lib/iam-labels'
import { Badge } from '../../shared/ui/badge'
import { Button } from '../../shared/ui/button'
import { Dialog, DialogContent } from '../../shared/ui/dialog'
import { Input } from '../../shared/ui/input'
import { Select } from '../../shared/ui/select'

export function RolesPage() {
  const { t } = useTranslation()
  const auth = useAuth()
  const queryClient = useQueryClient()
  const [open, setOpen] = useState(false)
  const [editing, setEditing] = useState<Role>()
  const query = useQuery({ queryKey: ['roles'], queryFn: () => apiRequest<PageResponse<Role>>('/roles?pageSize=100') })
  const permissions = useQuery({ queryKey: ['permissions'], queryFn: () => apiRequest<Permission[]>('/permissions') })
  const columns = useMemo<Array<ColumnDef<Role>>>(() => [
    { accessorKey: 'name', header: t('table.role'), cell: ({ row }) => <EntityCell detail={row.original.code} icon={KeyRound} name={roleLabel(t, row.original.code, row.original.name)} /> },
    { accessorKey: 'memberCount', header: t('common.members') },
    { accessorKey: 'dataScope', header: t('pages.roles.dataScope'), cell: ({ row }) => dataScopeLabel(t, row.original.dataScope) },
    { id: 'permissions', header: t('pages.roles.permissions'), cell: ({ row }) => <div className="flex max-w-xl flex-wrap gap-1">{row.original.permissions.slice(0, 4).map((key) => <Badge key={key}>{permissionLabel(t, key)}</Badge>)}{row.original.permissions.length > 4 && <Badge>+{row.original.permissions.length - 4}</Badge>}</div> },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status === 'active' ? 'active' : 'inactive'} /> },
    { id: 'actions', header: t('common.actions'), cell: ({ row }) => auth.hasPermission('role:manage') && !row.original.isBuiltin ? <Button onClick={() => { setEditing(row.original); setOpen(true) }} size="sm" variant="ghost">{t('common.edit')}</Button> : null },
  ], [auth, t])
  return <PageContainer>
    <PageHeader action={auth.hasPermission('role:manage') ? <Button onClick={() => { setEditing(undefined); setOpen(true) }}><Plus className="size-4" />{t('pages.roles.create')}</Button> : undefined} description={t('pages.roles.description')} title={t('pages.roles.title')} />
    {query.error || permissions.error ? <p className="mt-5 text-xs text-danger">{String(query.error ?? permissions.error)}</p> : <DataTable columns={columns} data={query.data?.items ?? []} getSearchText={(row) => `${row.name} ${row.code} ${row.permissions.join(' ')}`} getStatus={(row) => row.status === 'active' ? 'active' : 'inactive'} searchPlaceholder={t('pages.roles.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'inactive', label: t('common.inactive') }]} />}
    <RoleDialog available={permissions.data ?? []} key={editing?.id ?? 'new'} onClose={() => { setOpen(false); setEditing(undefined) }} onSaved={async () => { await queryClient.invalidateQueries({ queryKey: ['roles'] }) }} open={open} role={editing} />
  </PageContainer>
}

function RoleDialog({ role, available, open, onClose, onSaved }: { role?: Role; available: Permission[]; open: boolean; onClose: () => void; onSaved: () => Promise<void> }) {
  const { t } = useTranslation()
  const [code, setCode] = useState(role?.code ?? '')
  const [name, setName] = useState(role?.name ?? '')
  const [scope, setScope] = useState(role?.dataScope ?? 'own')
  const [selected, setSelected] = useState<string[]>(role?.permissions ?? [])
  const [error, setError] = useState('')
  const [pending, setPending] = useState(false)
  const submit = async (event: FormEvent) => {
    event.preventDefault()
    setPending(true)
    setError('')
    try {
      if (role) await apiRequest(`/roles/${role.id}`, { method: 'PATCH', body: jsonBody({ name, dataScope: scope, permissions: selected, version: role.version }) })
      else await apiRequest('/roles', { method: 'POST', body: jsonBody({ code, name, dataScope: scope, permissions: selected }) })
      onClose()
      void onSaved()
    } catch (value) { setError(String(value)) } finally { setPending(false) }
  }
  const toggle = (key: string) => setSelected((value) => value.includes(key) ? value.filter((item) => item !== key) : [...value, key])
  const title = role ? t('common.edit') : t('pages.roles.create')
  return <Dialog onOpenChange={(value) => { if (!value) onClose() }} open={open}>
    <DialogContent className="p-6" title={title}>
      <h3 className="text-lg font-semibold">{title}</h3>
      <form className="mt-5 space-y-4" onSubmit={(event) => void submit(event)}>
        <div className="grid grid-cols-2 gap-4">
          <label className="text-xs"><span className="mb-2 block">{t('common.name')}</span><Input onChange={(event) => setName(event.target.value)} required value={name} /></label>
          <label className="text-xs"><span className="mb-2 block">{t('pages.roles.code')}</span><Input disabled={Boolean(role)} onChange={(event) => setCode(event.target.value)} required value={code} /></label>
        </div>
        <label className="block text-xs"><span className="mb-2 block">{t('pages.roles.dataScope')}</span><Select className="w-full" onValueChange={setScope} options={['company', 'department_tree', 'own'].map((value) => ({ value, label: dataScopeLabel(t, value) }))} value={scope} /></label>
        <fieldset><legend className="mb-2 text-xs font-medium">{t('pages.roles.permissions')}</legend><div className="grid max-h-60 grid-cols-2 gap-2 overflow-auto rounded-lg border border-border p-3">{available.map((permission) => <label className="flex items-start gap-2 rounded-lg p-2 text-xs hover:bg-muted/50" key={permission.key}><input checked={selected.includes(permission.key)} className="mt-0.5 accent-primary" onChange={() => toggle(permission.key)} type="checkbox" /><span><strong className="block font-medium">{permissionLabel(t, permission.key)}</strong><span className="mt-0.5 block text-[10px] text-muted-foreground">{permission.key}</span></span></label>)}</div></fieldset>
        {error && <p className="text-xs text-danger">{error}</p>}
        <div className="flex justify-end gap-2"><Button onClick={onClose} type="button" variant="ghost">{t('common.cancel')}</Button><Button disabled={pending} type="submit">{t('common.save')}</Button></div>
      </form>
    </DialogContent>
  </Dialog>
}
