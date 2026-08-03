import type { ColumnDef } from '@tanstack/react-table'
import { Plus } from 'lucide-react'
import type { ReactNode } from 'react'

import { ComingSoonAction } from './coming-soon-action'
import { DataTable } from './data-table'
import { PageContainer } from './page-container'
import { PageHeader } from './page-header'

type ListPageProps<T> = {
  title: string
  description: string
  action?: ReactNode
  actionLabel?: string
  searchPlaceholder: string
  columns: Array<ColumnDef<T>>
  data: T[]
  getSearchText: (row: T) => string
  getStatus?: (row: T) => string
  statusOptions?: Array<{ value: string; label: string }>
  tableHeader?: ReactNode
}

export function ListPage<T>({ title, description, action, actionLabel, ...tableProps }: ListPageProps<T>) {
  return (
    <PageContainer>
      <PageHeader action={action ?? (actionLabel ? <ComingSoonAction><Plus className="size-4" />{actionLabel}</ComingSoonAction> : undefined)} description={description} title={title} />
      <DataTable {...tableProps} />
    </PageContainer>
  )
}
