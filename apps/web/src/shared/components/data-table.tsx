import {
  flexRender,
  getCoreRowModel,
  getPaginationRowModel,
  useReactTable,
  type ColumnDef,
} from '@tanstack/react-table'
import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Card } from '../ui/card'
import { Pagination } from '../ui/pagination'
import { SearchField } from '../ui/search-field'
import { Select } from '../ui/select'
import { Table, TableCell, TableContainer, TableHead } from '../ui/table'
import { EmptyState } from './empty-state'

type StatusOption = { value: string; label: string }

type DataTableProps<T> = {
  columns: Array<ColumnDef<T>>
  data: T[]
  getSearchText: (row: T) => string
  searchPlaceholder: string
  getStatus?: (row: T) => string
  statusOptions?: StatusOption[]
}

export function DataTable<T>({ columns, data, getSearchText, searchPlaceholder, getStatus, statusOptions = [] }: DataTableProps<T>) {
  const { t } = useTranslation()
  const [search, setSearch] = useState('')
  const [status, setStatus] = useState('all')
  const filteredData = useMemo(() => {
    const query = search.trim().toLocaleLowerCase()
    return data.filter((row) => {
      const matchesSearch = !query || getSearchText(row).toLocaleLowerCase().includes(query)
      const matchesStatus = status === 'all' || getStatus?.(row) === status
      return matchesSearch && matchesStatus
    })
  }, [data, getSearchText, getStatus, search, status])

  const table = useReactTable({
    columns,
    data: filteredData,
    getCoreRowModel: getCoreRowModel(),
    getPaginationRowModel: getPaginationRowModel(),
    initialState: { pagination: { pageIndex: 0, pageSize: 8 } },
  })

  useEffect(() => table.setPageIndex(0), [search, status, table])

  return (
    <Card className="mt-6 overflow-hidden">
      <div className="flex min-h-16 items-center gap-3 border-b border-border px-5">
        <SearchField onChange={(event) => setSearch(event.target.value)} placeholder={searchPlaceholder} value={search} />
        {statusOptions.length > 0 && (
          <Select aria-label={t('common.status')} onChange={(event) => setStatus(event.target.value)} value={status}>
            <option value="all">{t('common.all')}</option>
            {statusOptions.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
          </Select>
        )}
      </div>
      {table.getRowModel().rows.length === 0 ? <EmptyState /> : (
        <TableContainer>
          <Table>
            <thead className="bg-muted/45 text-[10px] uppercase tracking-[0.08em] text-muted-foreground">
              {table.getHeaderGroups().map((headerGroup) => (
                <tr key={headerGroup.id}>{headerGroup.headers.map((header) => <TableHead key={header.id}>{header.isPlaceholder ? null : flexRender(header.column.columnDef.header, header.getContext())}</TableHead>)}</tr>
              ))}
            </thead>
            <tbody>
              {table.getRowModel().rows.map((row) => (
                <tr className="border-t border-border transition-colors hover:bg-muted/30" key={row.id}>
                  {row.getVisibleCells().map((cell) => <TableCell key={cell.id}>{flexRender(cell.column.columnDef.cell, cell.getContext())}</TableCell>)}
                </tr>
              ))}
            </tbody>
          </Table>
        </TableContainer>
      )}
      <div className="flex min-h-15 items-center justify-end border-t border-border px-5">
        <Pagination page={table.getState().pagination.pageIndex} pageCount={Math.max(table.getPageCount(), 1)} onPageChange={table.setPageIndex} />
      </div>
    </Card>
  )
}
