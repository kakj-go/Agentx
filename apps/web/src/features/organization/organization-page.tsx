import type { ColumnDef } from '@tanstack/react-table'
import { Building2, ChevronRight, UserPlus, UsersRound } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ComingSoonAction } from '../../shared/components/coming-soon-action'
import { DataTable } from '../../shared/components/data-table'
import { EntityCell } from '../../shared/components/entity-cell'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { StatusBadge, type StatusValue } from '../../shared/components/status-badge'
import { cn } from '../../shared/lib/cn'
import { Card } from '../../shared/ui/card'

type UserRow = { name: string; account: string; department: string; departmentId: string; roles: string; status: StatusValue }

export function OrganizationPage() {
  const { t } = useTranslation()
  const [department, setDepartment] = useState('all')
  const departments = useMemo(() => [
    { id: 'all', label: t('pages.organization.allDepartments'), count: 42 },
    { id: 'ai', label: t('mocks.department.ai'), count: 12 },
    { id: 'service', label: t('mocks.department.service'), count: 14 },
    { id: 'sales', label: t('mocks.department.sales'), count: 10 },
    { id: 'legal', label: t('mocks.department.legal'), count: 6 },
  ], [t])
  const allUsers = useMemo<UserRow[]>(() => [
    { name: t('mocks.user.lin'), account: 'lin.xiao@xinghai.ai', department: t('mocks.department.ai'), departmentId: 'ai', roles: t('mocks.role.admin'), status: 'active' },
    { name: t('mocks.user.chen'), account: 'chen.yifan@xinghai.ai', department: t('mocks.department.service'), departmentId: 'service', roles: t('mocks.role.developer'), status: 'active' },
    { name: t('mocks.user.zhou'), account: 'zhou.yu@xinghai.ai', department: t('mocks.department.legal'), departmentId: 'legal', roles: t('mocks.role.reviewer'), status: 'active' },
    { name: t('mocks.user.wang'), account: 'wang.lei@xinghai.ai', department: t('mocks.department.sales'), departmentId: 'sales', roles: t('mocks.role.developer'), status: 'active' },
    { name: t('mocks.user.li'), account: 'li.ning@xinghai.ai', department: t('mocks.department.ai'), departmentId: 'ai', roles: t('mocks.role.viewer'), status: 'inactive' },
  ], [t])
  const data = department === 'all' ? allUsers : allUsers.filter((user) => user.departmentId === department)
  const columns = useMemo<Array<ColumnDef<UserRow>>>(() => [
    { accessorKey: 'name', header: t('pages.organization.users'), cell: ({ row }) => <EntityCell detail={row.original.account} icon={UsersRound} name={row.original.name} /> },
    { accessorKey: 'account', header: t('pages.organization.account') },
    { accessorKey: 'department', header: t('pages.organization.department') },
    { accessorKey: 'roles', header: t('pages.organization.roles') },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={row.original.status} /> },
  ], [t])

  return (
    <PageContainer>
      <PageHeader action={<ComingSoonAction><UserPlus className="size-4" />{t('pages.organization.invite')}</ComingSoonAction>} description={t('pages.organization.description')} title={t('pages.organization.title')} />
      <div className="mt-6 grid grid-cols-[240px_minmax(0,1fr)] gap-5">
        <Card className="h-fit overflow-hidden">
          <div className="flex h-14 items-center gap-2 border-b border-border px-4 text-sm font-semibold"><Building2 className="size-4 text-primary" />{t('pages.organization.departments')}</div>
          <div className="p-2">
            {departments.map((item) => (
              <button className={cn('flex h-10 w-full items-center gap-2 rounded-lg px-3 text-left text-xs text-muted-foreground hover:bg-muted hover:text-foreground', department === item.id && 'bg-primary/10 font-semibold text-primary hover:bg-primary/10 hover:text-primary')} key={item.id} onClick={() => setDepartment(item.id)}>
                <ChevronRight className="size-3.5" /><span className="flex-1">{item.label}</span><span className="text-[10px]">{item.count}</span>
              </button>
            ))}
          </div>
        </Card>
        <div className="min-w-0 [&>div]:mt-0">
          <DataTable columns={columns} data={data} getSearchText={(row) => `${row.name} ${row.account} ${row.department} ${row.roles}`} getStatus={(row) => row.status} searchPlaceholder={t('common.search')} statusOptions={[{ value: 'active', label: t('common.active') }, { value: 'inactive', label: t('common.inactive') }]} />
        </div>
      </div>
    </PageContainer>
  )
}
