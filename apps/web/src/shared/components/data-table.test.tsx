import type { ColumnDef } from '@tanstack/react-table'
import { fireEvent, render, screen } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { describe, expect, it } from 'vitest'

import { i18n } from '../../app/i18n'
import { DataTable } from './data-table'

type Row = { name: string; status: string }
const columns: Array<ColumnDef<Row>> = [{ accessorKey: 'name', header: 'Name' }, { accessorKey: 'status', header: 'Status' }]
const data = [{ name: 'Alpha', status: 'active' }, { name: 'Beta', status: 'inactive' }]

describe('DataTable', () => {
  it('filters local data by search and status', () => {
    render(<I18nextProvider i18n={i18n}><DataTable columns={columns} data={data} getSearchText={(row) => row.name} getStatus={(row) => row.status} searchPlaceholder="Find" statusOptions={[{ value: 'active', label: 'Active' }, { value: 'inactive', label: 'Inactive' }]} /></I18nextProvider>)
    fireEvent.change(screen.getByPlaceholderText('Find'), { target: { value: 'Alpha' } })
    expect(screen.getByText('Alpha')).toBeInTheDocument()
    expect(screen.queryByText('Beta')).not.toBeInTheDocument()
    fireEvent.change(screen.getByRole('combobox'), { target: { value: 'inactive' } })
    expect(screen.getByText(i18n.t('common.noResults'))).toBeInTheDocument()
  })

  it('paginates after eight rows', () => {
    const pageData = Array.from({ length: 9 }, (_, index) => ({ name: `Row ${index + 1}`, status: 'active' }))
    render(<I18nextProvider i18n={i18n}><DataTable columns={columns} data={pageData} getSearchText={(row) => row.name} searchPlaceholder="Find rows" /></I18nextProvider>)
    expect(screen.queryByText('Row 9')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: i18n.t('common.next') }))
    expect(screen.getByText('Row 9')).toBeInTheDocument()
  })
})
