import { useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { KeyRound, Plus } from 'lucide-react'
import { useMemo, useRef, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'

import { useAuth } from '../../app/providers/auth-provider'
import { apiFieldErrors, apiRequest, jsonBody } from '../../shared/api/client'
import type { PageResponse, Permission, Role } from '../../shared/api/types'
import { DataTable } from '../../shared/components/data-table'
import { EntityDeleteButton } from '../../shared/components/entity-delete-button'
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
import { useToast } from '../../shared/ui/toast'

export function RolesPage() {
  const { t } = useTranslation()
  const auth = useAuth()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const [open, setOpen] = useState(false)
  const [editing, setEditing] = useState<Role>()
  const query = useQuery({ queryKey: ['roles'], queryFn: () => apiRequest<PageResponse<Role>>('/roles?pageSize=100') })
  const permissions = useQuery({ queryKey: ['permissions'], queryFn: () => apiRequest<Permission[]>('/permissions') })
  const columns = useMemo<Array<ColumnDef<Role>>>(() => [
    { accessorKey: 'name', header: t('roles.title'), cell: ({ row }) => <EntityCell detail={row.original.code} icon={KeyRound} name={roleLabel(t, row.original.code, row.original.name)} /> },
    { accessorKey: 'memberCount', header: t('common.members') },
    { accessorKey: 'dataScope', header: t('roles.dataScope'), cell: ({ row }) => dataScopeLabel(t, row.original.dataScope) },
    { id: 'permissions', header: t('roles.permissions'), cell: ({ row }) => <div className="flex max-w-xl flex-wrap gap-1">{row.original.permissions.slice(0, 4).map((key) => <Badge key={key}>{permissionLabel(t, key)}</Badge>)}{row.original.permissions.length > 4 && <Badge>+{row.original.permissions.length - 4}</Badge>}</div> },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status === 'active' ? 'active' : 'inactive'} /> },
    { id: 'actions', header: t('common.actions'), cell: ({ row }) => <div className="flex gap-1">{auth.hasPermission('role:manage') && !row.original.isBuiltin && <Button onClick={() => { setEditing(row.original); setOpen(true) }} size="sm" variant="ghost">{t('common.edit')}</Button>}<EntityDeleteButton canDelete={auth.hasPermission('role:delete')} deletePath={`/roles/${row.original.id}`} entityId={row.original.id} entityName={row.original.name} entityType="role" immutableReason={row.original.isBuiltin ? t('common.deletion.immutable') : undefined} onDeleted={async () => { await queryClient.invalidateQueries({ queryKey: ['roles'] }); showToast(t('roles.deleted')) }} /></div> },
  ], [auth, queryClient, showToast, t])
  return <PageContainer>
    <PageHeader action={auth.hasPermission('role:manage') ? <Button onClick={() => { setEditing(undefined); setOpen(true) }}><Plus className="size-4" />{t('roles.create')}</Button> : undefined} description={t('roles.description')} title={t('roles.title')} />
    {query.error || permissions.error ? <p className="mt-5 text-xs text-danger">{String(query.error ?? permissions.error)}</p> : <DataTable columns={columns} data={query.data?.items ?? []} getSearchText={(row) => `${row.name} ${row.code} ${row.permissions.join(' ')}`} getStatus={(row) => row.status === 'active' ? 'active' : 'inactive'} searchPlaceholder={t('roles.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'inactive', label: t('common.inactive') }]} />}
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
  const [codeError, setCodeError] = useState('')
  const [pending, setPending] = useState(false)
  const formRef = useRef<HTMLFormElement>(null)
  const submit = async (event: FormEvent) => {
    event.preventDefault()
    setPending(true)
    setError('')
    setCodeError('')
    try {
      if (role) await apiRequest(`/roles/${role.id}`, { method: 'PATCH', body: jsonBody({ name, dataScope: scope, permissions: selected, version: role.version }) })
      else await apiRequest('/roles', { method: 'POST', body: jsonBody({ code, name, dataScope: scope, permissions: selected }) })
      onClose()
      void onSaved()
    } catch (value) {
      const fields = apiFieldErrors(value)
      setCodeError(fields.code ?? '')
      if (!fields.code) setError(String(value))
      if (fields.code) requestAnimationFrame(() => formRef.current?.querySelector<HTMLElement>('[name="code"]')?.focus())
    } finally { setPending(false) }
  }
  const toggle = (key: string) => setSelected((value) => value.includes(key) ? value.filter((item) => item !== key) : [...value, key])
  const title = role ? t('common.edit') : t('roles.create')
  return <Dialog onOpenChange={(value) => { if (!value) onClose() }} open={open}>
    <DialogContent className="p-6" title={title}>
      <h3 className="text-lg font-semibold">{title}</h3>
      <form className="mt-5 space-y-4" onSubmit={(event) => void submit(event)} ref={formRef}>
        <div className="grid grid-cols-2 gap-4">
          <label className="text-xs"><span className="mb-2 block">{t('common.name')}<RequiredMark /></span><Input aria-label={t('common.name')} onChange={(event) => setName(event.target.value)} required value={name} /></label>
          <label className="text-xs"><span className="mb-2 block">{t('roles.code')}<RequiredMark /></span><Input aria-describedby={codeError ? 'role-code-error' : undefined} aria-invalid={Boolean(codeError)} aria-label={t('roles.code')} disabled={Boolean(role)} name="code" onChange={(event) => { setCode(event.target.value); setCodeError('') }} required value={code} />{codeError && <span className="mt-1.5 block text-danger" id="role-code-error">{codeError}</span>}</label>
        </div>
        <label className="block text-xs"><span className="mb-2 block">{t('roles.dataScope')}<RequiredMark /></span><Select aria-label={t('roles.dataScope')} aria-required className="w-full" onValueChange={setScope} options={['company', 'department_tree', 'own'].map((value) => ({ value, label: dataScopeLabel(t, value) }))} value={scope} /></label>
        <fieldset><legend className="mb-2 text-xs font-medium">{t('roles.permissions')}</legend><div className="grid max-h-60 grid-cols-2 gap-2 overflow-auto rounded-lg border border-border p-3">{available.map((permission) => <label className="flex items-start gap-2 rounded-lg p-2 text-xs hover:bg-muted/50" key={permission.key}><input checked={selected.includes(permission.key)} className="mt-0.5 accent-primary" onChange={() => toggle(permission.key)} type="checkbox" /><span><strong className="block font-medium">{permissionLabel(t, permission.key)}</strong><span className="mt-0.5 block text-[10px] text-muted-foreground">{permission.key}</span></span></label>)}</div></fieldset>
        {error && <p className="text-xs text-danger">{error}</p>}
        <div className="flex justify-end gap-2"><Button onClick={onClose} type="button" variant="ghost">{t('common.cancel')}</Button><Button disabled={pending} type="submit">{t('common.save')}</Button></div>
      </form>
    </DialogContent>
  </Dialog>
}

function RequiredMark() { return <span aria-hidden="true" className="ml-1 text-danger">*</span> }
