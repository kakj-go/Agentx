import { useQuery } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { ShieldCheck } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useSearchParams } from 'react-router-dom'

import { apiRequest } from '../../shared/api/client'
import type { Approval, PageResponse, ResourceGrantRequest } from '../../shared/api/types'
import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge, type StatusValue } from '../../shared/components/status-badge'
import { useLocaleFormat } from '../../shared/lib/locale-format'
import { localizedValue } from '../../shared/lib/localized-value'
import { Button } from '../../shared/ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../../shared/ui/tabs'
import { approvalStatus } from './approval-status'

export function ApprovalsPage() {
  const { t } = useTranslation()
  const { formatDateTime } = useLocaleFormat()
  const [searchParams, setSearchParams] = useSearchParams()
  const activeTab = searchParams.get('tab') === 'resource-grants' ? 'resource-grants' : 'runtime'
  const approvals = useQuery({ queryKey: ['approvals'], queryFn: () => apiRequest<PageResponse<Approval>>('/approvals?pageSize=100') })
  const resourceRequests = useQuery({ queryKey: ['resource-grant-requests'], queryFn: () => apiRequest<PageResponse<ResourceGrantRequest>>('/resource-grant-requests?pageSize=100') })
  const columns = useMemo<Array<ColumnDef<Approval>>>(() => [
    { accessorKey: 'title', header: t('common.name'), cell: ({ row }) => <EntityCell detail={row.original.workflowName} icon={ShieldCheck} name={row.original.title} /> },
    { accessorKey: 'workflowName', header: t('approvals.workflow') },
    { accessorKey: 'claimedByName', header: t('approvals.assignee'), cell: ({ row }) => row.original.claimedByName ?? '—' },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge status={approvalStatus(row.original.status)} /> },
    { accessorKey: 'deadlineAt', header: t('approvals.deadline'), cell: ({ row }) => formatDateTime(row.original.deadlineAt) },
    { accessorKey: 'createdAt', header: t('approvals.createdAt'), cell: ({ row }) => formatDateTime(row.original.createdAt) },
    { id: 'actions', header: '', cell: ({ row }) => <Button asChild size="sm" variant="ghost"><Link to={`/approvals/${row.original.id}`}>{t('approvals.review')}</Link></Button> },
  ], [formatDateTime, t])
  const requestColumns = useMemo<Array<ColumnDef<ResourceGrantRequest>>>(() => [
    { accessorKey: 'primaryResourceName', header: t('common.name'), cell: ({ row }) => <EntityCell detail={`${localizedValue(t, 'resourceGrants.resourceTypes', row.original.primaryResourceType)} · ${localizedValue(t, 'resourceGrants.operations', row.original.operation)}`} icon={ShieldCheck} name={row.original.primaryResourceName ?? t('approvals.controlledResource')} /> },
    { id: 'subject', header: t('approvals.workflow'), cell: ({ row }) => row.original.workflowName ?? row.original.subjectDepartmentName ?? row.original.subjectId },
    { accessorKey: 'requestedByName', header: t('approvals.requester') },
    { accessorKey: 'reviews', header: t('approvals.reviewProgress'), cell: ({ row }) => `${row.original.reviews.filter((review) => review.status === 'approved').length}/${row.original.reviews.length}` },
    { accessorKey: 'status', header: t('common.status'), cell: ({ row }) => <StatusBadge label={localizedValue(t, 'approvals.resourceGrantStatuses', row.original.status)} status={resourceRequestStatus(row.original.status)} /> },
    { accessorKey: 'updatedAt', header: t('common.updatedAt'), cell: ({ row }) => formatDateTime(row.original.updatedAt) },
    { id: 'actions', header: '', cell: ({ row }) => <Button asChild size="sm" variant="ghost"><Link to={`/approvals/resource-grants/${row.original.id}`}>{t('approvals.review')}</Link></Button> },
  ], [formatDateTime, t])
  const tabs = <TabsList className="border-b border-border"><TabsTrigger value="runtime">{t('approvals.runtimeTab')}</TabsTrigger><TabsTrigger value="resource-grants">{t('approvals.resourceGrantTab')}</TabsTrigger></TabsList>
  return <Tabs onValueChange={(value) => setSearchParams(value === 'resource-grants' ? { tab: value } : {})} value={activeTab}>
    <TabsContent value="runtime"><ListPage columns={columns} data={approvals.data?.items ?? []} description={t('approvals.description')} getSearchText={(row) => `${row.title} ${row.workflowName} ${row.claimedByName ?? ''}`} getStatus={(row) => approvalStatus(row.status)} searchPlaceholder={t('approvals.search')} statusOptions={[{ value: 'pending', label: t('common.pending') }, { value: 'waiting', label: t('common.waiting') }, { value: 'completed', label: t('common.completed') }, { value: 'inactive', label: t('common.inactive') }]} tableHeader={tabs} title={t('approvals.title')} /></TabsContent>
    <TabsContent value="resource-grants"><ListPage columns={requestColumns} data={resourceRequests.data?.items ?? []} description={t('approvals.resourceGrantDescription')} getSearchText={(row) => `${row.primaryResourceName ?? ''} ${row.primaryResourceType} ${row.workflowName ?? row.subjectDepartmentName ?? row.subjectId} ${row.requestedByName}`} getStatus={(row) => resourceRequestStatus(row.status)} searchPlaceholder={t('approvals.resourceGrantSearch')} statusOptions={[{ value: 'pending', label: t('common.pending') }, { value: 'active', label: t('approvals.resourceGrantStatuses.approved') }, { value: 'inactive', label: t('common.inactive') }]} tableHeader={tabs} title={t('approvals.title')} /></TabsContent>
  </Tabs>
}

function resourceRequestStatus(status: string): StatusValue {
  if (status === 'approved') return 'active'
  if (status === 'pending') return 'pending'
  return 'inactive'
}
