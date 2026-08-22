import * as Popover from '@radix-ui/react-popover'
import { useQuery } from '@tanstack/react-query'
import { Check, ChevronDown, Search } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '../ui/button'
import { Input } from '../ui/input'

export type MultiSelectOption = { value: string; label: string; detail?: string }

type Props = {
  label: string
  queryKey: string
  value: string[]
  onChange: (value: string[], selected?: MultiSelectOption) => void
  loadOptions?: (search: string) => Promise<MultiSelectOption[]>
  options?: MultiSelectOption[]
}

export function SearchableMultiSelect({ label, queryKey, value, onChange, loadOptions, options = [] }: Props) {
  const { t } = useTranslation()
  const [open, setOpen] = useState(false)
  const [search, setSearch] = useState('')
  const [debouncedSearch, setDebouncedSearch] = useState('')
  useEffect(() => {
    const timeout = window.setTimeout(() => setDebouncedSearch(search.trim()), 300)
    return () => window.clearTimeout(timeout)
  }, [search])
  const remote = useQuery({
    queryKey: ['multi-select-options', queryKey, debouncedSearch],
    enabled: open && Boolean(loadOptions),
    queryFn: () => loadOptions?.(debouncedSearch) ?? Promise.resolve([]),
  })
  const visible = loadOptions ? remote.data ?? [] : options.filter((option) => `${option.label} ${option.detail ?? ''}`.toLocaleLowerCase().includes(debouncedSearch.toLocaleLowerCase()))
  return <Popover.Root onOpenChange={(next) => { setOpen(next); if (!next) setSearch('') }} open={open}>
    <Popover.Trigger asChild>
      <Button aria-label={label} size="sm" variant="secondary">{label}{value.length > 0 && <span className="rounded-full bg-primary/10 px-1.5 text-[10px] text-primary">{value.length}</span>}<ChevronDown className="size-3.5" /></Button>
    </Popover.Trigger>
    <Popover.Portal>
      <Popover.Content align="start" className="z-[120] w-72 overflow-hidden rounded-lg border border-border bg-surface shadow-xl" sideOffset={5}>
        <div className="relative border-b border-border p-2"><Search className="pointer-events-none absolute left-4 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" /><Input aria-label={t('common.search')} className="h-8 pl-8 text-xs" onChange={(event) => setSearch(event.target.value)} value={search} /></div>
        <div className="max-h-64 overflow-y-auto p-1" role="listbox">
          {remote.isLoading && <p className="p-3 text-xs text-muted-foreground">{t('common.loading')}</p>}
          {remote.isError && <p className="p-3 text-xs text-danger">{t('executions.filterLoadFailed')}</p>}
          {!remote.isLoading && !remote.isError && visible.length === 0 && <p className="p-3 text-xs text-muted-foreground">{t('common.noResults')}</p>}
          {visible.map((option) => {
            const selected = value.includes(option.value)
            return <button aria-selected={selected} className="flex min-h-10 w-full items-center gap-2 rounded-md px-2 text-left text-xs hover:bg-muted/45" key={option.value} onClick={() => onChange(selected ? value.filter((item) => item !== option.value) : [...value, option.value], option)} role="option" type="button">
              <span className={`flex size-4 shrink-0 items-center justify-center rounded border ${selected ? 'border-primary bg-primary text-primary-foreground' : 'border-border'}`}>{selected && <Check className="size-3" />}</span>
              <span className="min-w-0"><span className="block truncate text-foreground">{option.label}</span>{option.detail && <span className="block truncate text-[10px] text-muted-foreground">{option.detail}</span>}</span>
            </button>
          })}
        </div>
      </Popover.Content>
    </Popover.Portal>
  </Popover.Root>
}
