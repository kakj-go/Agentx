import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { Building2, ChevronDown, ChevronRight, Pencil, Plus, UserPlus, UsersRound } from 'lucide-react'
import { useMemo, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, jsonBody } from '../../shared/api/client'
import type { Department, PageResponse, Role, User } from '../../shared/api/types'
import { DataTable } from '../../shared/components/data-table'
import { EntityCell } from '../../shared/components/entity-cell'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { StatusBadge } from '../../shared/components/status-badge'
import { cn } from '../../shared/lib/cn'
import { roleLabel } from '../../shared/lib/iam-labels'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { Dialog, DialogContent } from '../../shared/ui/dialog'
import { Input } from '../../shared/ui/input'
import { Select, type SelectOption } from '../../shared/ui/select'
import { useToast } from '../../shared/ui/toast'

type DepartmentNode = Department & { children: DepartmentNode[] }

function buildDepartmentTree(departments: Department[]) {
  const nodes = new Map(departments.map((item) => [item.id, { ...item, children: [] } as DepartmentNode]))
  const roots: DepartmentNode[] = []
  for (const node of nodes.values()) {
    const parent = node.parentId ? nodes.get(node.parentId) : undefined
    if (parent) parent.children.push(node)
    else roots.push(node)
  }
  return roots
}

function descendantIds(departments: Department[], parentId: string) {
  const ids = new Set([parentId])
  let changed = true
  while (changed) {
    changed = false
    for (const item of departments) {
      if (item.parentId && ids.has(item.parentId) && !ids.has(item.id)) {
        ids.add(item.id)
        changed = true
      }
    }
  }
  return ids
}

function departmentOptions(departments: Department[], excludedIds = new Set<string>()): SelectOption[] {
  const options: SelectOption[] = []
  const visit = (nodes: DepartmentNode[], depth: number) => {
    for (const node of nodes) {
      if (!excludedIds.has(node.id)) options.push({ value: node.id, label: `${'　'.repeat(depth)}${node.name}` })
      visit(node.children, depth + 1)
    }
  }
  visit(buildDepartmentTree(departments), 0)
  return options
}

export function OrganizationPage() {
  const { t } = useTranslation()
  const auth = useAuth()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const [selectedDepartment, setSelectedDepartment] = useState('all')
  const [departmentDialog, setDepartmentDialog] = useState<{ department?: Department; parentId?: string }>()
  const [editingUser, setEditingUser] = useState<User>()
  const [userOpen, setUserOpen] = useState(false)
  const departments = useQuery({ queryKey: ['departments'], queryFn: () => apiRequest<Department[]>('/departments') })
  const users = useQuery({ queryKey: ['users'], queryFn: () => apiRequest<PageResponse<User>>('/users?pageSize=100') })
  const roles = useQuery({ queryKey: ['roles'], queryFn: () => apiRequest<PageResponse<Role>>('/roles?pageSize=100') })
  const departmentList = useMemo(() => departments.data ?? [], [departments.data])
  const userList = useMemo(() => users.data?.items ?? [], [users.data])
  const rootDepartment = departmentList.find((item) => item.isRoot) ?? departmentList[0]

  const invalidate = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ['departments'] }),
      queryClient.invalidateQueries({ queryKey: ['users'] }),
      queryClient.invalidateQueries({ queryKey: ['roles'] }),
    ])
  }
  const disable = useMutation({
    mutationFn: (id: string) => apiRequest(`/users/${id}/disable`, { method: 'POST' }),
    onSuccess: () => void invalidate(),
    onError: (error: Error) => showToast(error.message),
  })
  const visibleIds = selectedDepartment === 'all' ? undefined : descendantIds(departmentList, selectedDepartment)
  const data = userList.filter((user) => !visibleIds || visibleIds.has(user.departmentId))
  const counts = useMemo(() => new Map(departmentList.map((item) => {
    const ids = descendantIds(departmentList, item.id)
    return [item.id, userList.filter((user) => ids.has(user.departmentId)).length]
  })), [departmentList, userList])
  const columns = useMemo<Array<ColumnDef<User>>>(() => [
    { accessorKey: 'displayName', header: t('pages.organization.users'), cell: ({ row }) => <EntityCell detail={row.original.username} icon={UsersRound} name={row.original.displayName} /> },
    { accessorKey: 'departmentName', header: t('pages.organization.department') },
    { id: 'roles', header: t('pages.organization.roles'), cell: ({ row }) => row.original.roles.map((role) => roleLabel(t, role)).join(', ') },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status === 'active' ? 'active' : row.original.status === 'invited' ? 'invited' : 'inactive'} /> },
    { id: 'actions', header: t('common.actions'), cell: ({ row }) => <div className="flex gap-1">
      {row.original.status !== 'disabled' && auth.hasPermission('user:update') && <Button onClick={() => { setEditingUser(row.original); setUserOpen(true) }} size="sm" variant="ghost">{t('common.edit')}</Button>}
      {row.original.status !== 'disabled' && auth.hasPermission('user:disable') && <Button disabled={row.original.id === auth.user?.id || disable.isPending} onClick={() => disable.mutate(row.original.id)} size="sm" variant="ghost">{t('pages.organization.disable')}</Button>}
    </div> },
  ], [auth, disable, t])

  const createDepartment = () => {
    const parentId = selectedDepartment === 'all' ? rootDepartment?.id : selectedDepartment
    setDepartmentDialog({ parentId })
  }
  const createUser = () => {
    setEditingUser(undefined)
    setUserOpen(true)
  }

  return <PageContainer>
    <PageHeader
      action={<div className="flex gap-2">
        {auth.hasPermission('department:manage') && <Button disabled={!rootDepartment} onClick={createDepartment} variant="secondary"><Plus className="size-4" />{t('pages.organization.addDepartment')}</Button>}
        {auth.hasPermission('user:create') && <Button onClick={createUser}><UserPlus className="size-4" />{t('pages.organization.createUser')}</Button>}
      </div>}
      description={t('pages.organization.description')}
      title={t('pages.organization.title')}
    />
    {(departments.error || users.error) && <p className="mt-5 rounded-lg border border-danger/20 bg-danger/10 p-3 text-xs text-danger">{String(departments.error ?? users.error)}</p>}
    <div className="mt-6 grid grid-cols-[280px_minmax(0,1fr)] gap-5">
      <Card className="h-fit overflow-hidden">
        <div className="flex h-14 items-center gap-2 border-b border-border px-4 text-sm font-semibold"><Building2 className="size-4 text-primary" />{t('pages.organization.departments')}</div>
        <div className="p-2">
          <DepartmentRow active={selectedDepartment === 'all'} count={users.data?.total ?? 0} label={t('pages.organization.allDepartments')} onSelect={() => setSelectedDepartment('all')} />
          {buildDepartmentTree(departmentList).map((node) => <DepartmentTreeRow
            activeId={selectedDepartment}
            canEdit={auth.hasPermission('department:manage')}
            counts={counts}
            key={node.id}
            node={node}
            onEdit={(item) => setDepartmentDialog({ department: item })}
            onSelect={setSelectedDepartment}
          />)}
        </div>
      </Card>
      <div className="min-w-0 [&>div]:mt-0"><DataTable columns={columns} data={data} getSearchText={(row) => `${row.displayName} ${row.username} ${row.departmentName} ${row.roles.join(' ')}`} getStatus={(row) => row.status === 'active' ? 'active' : row.status === 'invited' ? 'invited' : 'inactive'} searchPlaceholder={t('common.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'invited', label: t('common.invited') }, { value: 'inactive', label: t('common.inactive') }]} /></div>
    </div>
    {departmentDialog && <DepartmentDialog
      department={departmentDialog?.department}
      departments={departmentList}
      key={`${departmentDialog?.department?.id ?? 'new'}-${departmentDialog?.parentId ?? ''}`}
      onClose={() => setDepartmentDialog(undefined)}
      onSaved={invalidate}
      open
      parentId={departmentDialog?.parentId}
    />}
    {userOpen && <UserDialog
      departments={departmentList}
      initialDepartmentId={selectedDepartment === 'all' ? rootDepartment?.id : selectedDepartment}
      key={editingUser?.id ?? `new-${selectedDepartment}`}
      onClose={() => { setUserOpen(false); setEditingUser(undefined) }}
      onSaved={invalidate}
      open
      roles={roles.data?.items ?? []}
      user={editingUser}
    />}
  </PageContainer>
}

function DepartmentTreeRow({ node, activeId, counts, canEdit, onSelect, onEdit }: { node: DepartmentNode; activeId: string; counts: Map<string, number>; canEdit: boolean; onSelect: (id: string) => void; onEdit: (department: Department) => void }) {
  const { t } = useTranslation()
  const [expanded, setExpanded] = useState(true)
  return <div>
    <DepartmentRow
      active={activeId === node.id}
      count={counts.get(node.id) ?? 0}
      expandable={node.children.length > 0}
      expanded={expanded}
      label={node.name}
      onEdit={!node.isRoot && canEdit ? () => onEdit(node) : undefined}
      onSelect={() => onSelect(node.id)}
      onToggle={() => setExpanded((value) => !value)}
      toggleLabel={t(expanded ? 'pages.organization.collapse' : 'pages.organization.expand')}
    />
    {expanded && node.children.length > 0 && <div className="ml-4 border-l border-border pl-1">{node.children.map((child) => <DepartmentTreeRow activeId={activeId} canEdit={canEdit} counts={counts} key={child.id} node={child} onEdit={onEdit} onSelect={onSelect} />)}</div>}
  </div>
}

function DepartmentRow({ active, count, label, onSelect, onEdit, expandable, expanded, onToggle, toggleLabel }: { active: boolean; count: number; label: string; onSelect: () => void; onEdit?: () => void; expandable?: boolean; expanded?: boolean; onToggle?: () => void; toggleLabel?: string }) {
  const ToggleIcon = expanded ? ChevronDown : ChevronRight
  return <div className={cn('group flex h-10 items-center rounded-lg text-xs text-muted-foreground hover:bg-muted hover:text-foreground', active && 'bg-primary/10 font-semibold text-primary hover:bg-primary/10 hover:text-primary')}>
    {expandable ? <button aria-label={toggleLabel} className="ml-1 grid size-7 shrink-0 place-items-center rounded-md hover:bg-background/70" onClick={onToggle}><ToggleIcon className="size-3.5" /></button> : <span className="ml-1 size-7 shrink-0" />}
    <button className="flex min-w-0 flex-1 items-center gap-2 pr-2 text-left" onClick={onSelect}><span className="flex-1 truncate">{label}</span><span className="text-[10px]">{count}</span></button>
    {onEdit && <button aria-label={label} className="mr-1 rounded p-1 opacity-0 hover:bg-background group-hover:opacity-100 focus-visible:opacity-100" onClick={onEdit}><Pencil className="size-3" /></button>}
  </div>
}

function DepartmentDialog({ department, departments, parentId, open, onClose, onSaved }: { department?: Department; departments: Department[]; parentId?: string; open: boolean; onClose: () => void; onSaved: () => Promise<void> }) {
  const { t } = useTranslation()
  const [name, setName] = useState(department?.name ?? '')
  const [parent, setParent] = useState(department?.parentId ?? parentId ?? '')
  const [error, setError] = useState('')
  const [pending, setPending] = useState(false)
  const excluded = department ? descendantIds(departments, department.id) : new Set<string>()
  const options = departmentOptions(departments, excluded)
  const parentName = departments.find((item) => item.id === parent)?.name
  const title = department ? t('pages.organization.editDepartment') : parentName ? t('pages.organization.addChildDepartment', { name: parentName }) : t('pages.organization.addDepartment')
  const submit = async (event: FormEvent) => {
    event.preventDefault()
    setPending(true)
    setError('')
    try {
      if (department) await apiRequest(`/departments/${department.id}`, { method: 'PATCH', body: jsonBody({ name, parentId: parent || null, version: department.version }) })
      else await apiRequest('/departments', { method: 'POST', body: jsonBody({ name, parentId: parent }) })
      onClose()
      void onSaved()
    } catch (value) { setError(String(value)) } finally { setPending(false) }
  }
  return <Dialog onOpenChange={(value) => { if (!value) onClose() }} open={open}>
    <DialogContent className="p-6" title={title}>
      <h3 className="text-lg font-semibold">{title}</h3>
      <form className="mt-5 space-y-4" onSubmit={(event) => void submit(event)}>
        <Field label={t('common.name')} onChange={setName} value={name} />
        <label className="block text-xs"><span className="mb-2 block">{t('pages.organization.parentDepartment')}</span><Select className="w-full" onValueChange={setParent} options={options} value={parent} /></label>
        {error && <p className="text-xs text-danger">{error}</p>}
        <div className="flex justify-end gap-2"><Button onClick={onClose} type="button" variant="ghost">{t('common.cancel')}</Button><Button disabled={pending || !parent} type="submit">{t('common.save')}</Button></div>
      </form>
    </DialogContent>
  </Dialog>
}

function UserDialog({ user, departments, roles, initialDepartmentId, open, onClose, onSaved }: { user?: User; departments: Department[]; roles: Role[]; initialDepartmentId?: string; open: boolean; onClose: () => void; onSaved: () => Promise<void> }) {
  const { t } = useTranslation()
  const [username, setUsername] = useState(user?.username ?? '')
  const [name, setName] = useState(user?.displayName ?? '')
  const [department, setDepartment] = useState(user?.departmentId ?? initialDepartmentId ?? '')
  const [role, setRole] = useState(roles.find((item) => item.code === user?.roles[0])?.id ?? roles.find((item) => item.code === 'member')?.id ?? '')
  const [error, setError] = useState('')
  const [pending, setPending] = useState(false)
  const selectableRoles = roles.filter((item) => item.code !== 'company_admin' || user?.roles.includes('company_admin'))
  const submit = async (event: FormEvent) => {
    event.preventDefault()
    setPending(true)
    setError('')
    try {
      if (user) await apiRequest(`/users/${user.id}`, { method: 'PATCH', body: jsonBody({ displayName: name, departmentId: department, roleId: role, version: user.version }) })
      else await apiRequest('/users', { method: 'POST', body: jsonBody({ username, displayName: name, departmentId: department, roleId: role }) })
      onClose()
      void onSaved()
    } catch (value) { setError(String(value)) } finally { setPending(false) }
  }
  const title = user ? t('pages.organization.editUser') : t('pages.organization.createUser')
  return <Dialog onOpenChange={(value) => { if (!value) onClose() }} open={open}>
    <DialogContent className="p-6" title={title}>
      <h3 className="text-lg font-semibold">{title}</h3>
      <form className="mt-5 grid grid-cols-2 gap-4" onSubmit={(event) => void submit(event)}>
        <Field disabled={Boolean(user)} label={t('auth.username')} onChange={setUsername} value={username} />
        <Field label={t('pages.organization.userName')} onChange={setName} value={name} />
        <label className="text-xs"><span className="mb-2 block">{t('pages.organization.department')}</span><Select className="w-full" onValueChange={setDepartment} options={departmentOptions(departments)} value={department} /></label>
        <label className="text-xs"><span className="mb-2 block">{t('pages.organization.roles')}</span><Select className="w-full" onValueChange={setRole} options={selectableRoles.map((item) => ({ value: item.id, label: roleLabel(t, item.code, item.name) }))} value={role} /></label>
        {!user && <div className="col-span-2 rounded-lg border border-primary/20 bg-primary/5 px-3 py-2 text-xs"><strong>{t('pages.organization.initialPassword')}: 123456</strong><p className="mt-1 text-[11px] text-muted-foreground">{t('pages.organization.initialPasswordHint')}</p></div>}
        {error && <p className="col-span-2 text-xs text-danger">{error}</p>}
        <div className="col-span-2 flex justify-end gap-2"><Button onClick={onClose} type="button" variant="ghost">{t('common.cancel')}</Button><Button disabled={pending || !department || !role} type="submit">{t('common.save')}</Button></div>
      </form>
    </DialogContent>
  </Dialog>
}

function Field({ label, value, onChange, disabled = false }: { label: string; value: string; onChange: (value: string) => void; disabled?: boolean }) {
  return <label className="text-xs"><span className="mb-2 block">{label}</span><Input disabled={disabled} onChange={(event) => onChange(event.target.value)} required value={value} /></label>
}
