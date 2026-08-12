import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { Download, Edit3, FilePlus2, Plus, Upload } from 'lucide-react'
import { useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, apiRequestText, jsonBody } from '../../shared/api/client'
import type { Dataset, DatasetCase, DatasetVersion } from '../../shared/api/types'
import { ConfirmDialog } from '../../shared/components/confirm-dialog'
import { DataTable } from '../../shared/components/data-table'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { EmptyState } from '../../shared/components/empty-state'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { PrerequisiteAction } from '../../shared/components/prerequisite-action'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../../shared/ui/tabs'
import { useToast } from '../../shared/ui/toast'

type FormKind = 'dataset' | 'case' | null

export function DatasetDetailPage() {
  const { id = '' } = useParams()
  const { t } = useTranslation()
  const auth = useAuth()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const inputRef = useRef<HTMLInputElement>(null)
  const [form, setForm] = useState<FormKind>(null)
  const [selectedCase, setSelectedCase] = useState<DatasetCase | null>(null)
  const [deleteCase, setDeleteCase] = useState<DatasetCase | null>(null)

  const dataset = useQuery({ queryKey: ['dataset', id], queryFn: () => apiRequest<Dataset>(`/datasets/${id}`) })
  const cases = useQuery({ queryKey: ['dataset-cases', id], queryFn: () => apiRequest<DatasetCase[]>(`/datasets/${id}/cases`) })
  const versions = useQuery({ queryKey: ['dataset-versions', id], queryFn: () => apiRequest<DatasetVersion[]>(`/datasets/${id}/versions`) })
  const invalidate = async () => { await Promise.all([queryClient.invalidateQueries({ queryKey: ['dataset', id] }), queryClient.invalidateQueries({ queryKey: ['dataset-cases', id] }), queryClient.invalidateQueries({ queryKey: ['dataset-versions', id] }), queryClient.invalidateQueries({ queryKey: ['datasets'] })]) }
  const save = useMutation<unknown, Error, { kind: Exclude<FormKind, null>; values: Record<string, string> }>({
    mutationFn: ({ kind, values }: { kind: Exclude<FormKind, null>; values: Record<string, string> }) => {
      if (kind === 'dataset') return apiRequest<Dataset>(`/datasets/${id}`, { method: 'PATCH', body: jsonBody({ name: values.name, description: values.description || null, visibility: values.visibility, status: values.status, version: dataset.data?.version }) })
      const body = { expectedRevision: dataset.data?.revision, caseKey: values.caseKey, name: values.name, input: JSON.parse(values.input), expectedOutput: values.expectedOutput ? JSON.parse(values.expectedOutput) : null, context: values.context ? JSON.parse(values.context) : null, tags: values.tags ? values.tags.split(',').map((item) => item.trim()).filter(Boolean) : [], evaluatorOverride: null }
      return selectedCase
        ? apiRequest<DatasetCase>(`/datasets/${id}/cases/${selectedCase.id}`, { method: 'PATCH', body: jsonBody({ ...body, version: selectedCase.version }) })
        : apiRequest<DatasetCase>(`/datasets/${id}/cases`, { method: 'POST', body: jsonBody(body) })
    },
    onSuccess: async () => { await invalidate(); showToast(t('datasets.saved')) },
  })
  const remove = useMutation({ mutationFn: (item: DatasetCase) => apiRequest(`/datasets/${id}/cases/${item.id}`, { method: 'DELETE', body: jsonBody({ expectedRevision: dataset.data?.revision }) }), onSuccess: async () => { setDeleteCase(null); await invalidate(); showToast(t('datasets.saved')) } })
  const publish = useMutation({ mutationFn: () => apiRequest<DatasetVersion>(`/datasets/${id}/versions`, { method: 'POST' }), onSuccess: async () => { await invalidate(); showToast(t('datasets.versionPublished')) } })
  const importFile = async (file: File) => {
    try {
      const content = await file.text()
      await apiRequest(`/datasets/${id}/import`, { method: 'POST', body: jsonBody({ expectedRevision: dataset.data?.revision, format: file.name.toLowerCase().endsWith('.csv') ? 'csv' : 'jsonl', content }) })
      await invalidate()
      showToast(t('datasets.imported'))
    } catch (error) { showToast(error instanceof Error ? error.message : String(error)) }
  }
  const exportFile = async () => { const content = await apiRequestText(`/datasets/${id}/export`); const url = URL.createObjectURL(new Blob([content], { type: 'application/x-ndjson' })); const anchor = document.createElement('a'); anchor.href = url; anchor.download = `${dataset.data?.name ?? 'dataset'}.jsonl`; anchor.click(); URL.revokeObjectURL(url) }
  const columns = useMemo<Array<ColumnDef<DatasetCase>>>(() => [
    { accessorKey: 'caseKey', header: t('datasets.caseKey') },
    { accessorKey: 'name', header: t('common.name') },
    { accessorKey: 'tags', header: t('datasets.tags'), cell: ({ row }) => row.original.tags.join(', ') || '—' },
    { accessorKey: 'version', header: t('common.version') },
    { id: 'actions', header: '', cell: ({ row }) => auth.hasPermission('dataset:manage') && <div className="flex justify-end"><Button aria-label={t('common.edit')} onClick={() => { setSelectedCase(row.original); setForm('case') }} size="icon" variant="ghost"><Edit3 className="size-4" /></Button><Button aria-label={t('datasets.deleteCase')} className="text-danger hover:bg-danger/10 hover:text-danger" onClick={() => setDeleteCase(row.original)} size="sm" variant="ghost">{t('common.delete')}</Button></div> },
  ], [auth, t])
  if (!dataset.data) return <PageContainer><EmptyState title={dataset.isLoading ? t('common.loading') : t('common.loadFailed')} description={String(dataset.error ?? '')} /></PageContainer>
  const datasetFields: EntityFormField[] = [
    { name: 'name', label: t('common.name'), required: true, defaultValue: dataset.data.name },
    { name: 'description', label: t('common.description'), type: 'textarea', defaultValue: dataset.data.description ?? '' },
    { name: 'visibility', label: t('datasets.visibility'), type: 'select', defaultValue: dataset.data.visibility, options: ['private', 'department', 'company'].map((item) => ({ value: item, label: t(`datasets.${item}`) })) },
    { name: 'status', label: t('common.status'), type: 'select', defaultValue: dataset.data.status, options: ['active', 'disabled'].map((item) => ({ value: item, label: t(`datasets.${item}`) })) },
  ]
  const caseFields: EntityFormField[] = [
    { name: 'caseKey', label: t('datasets.caseKey'), required: true, defaultValue: selectedCase?.caseKey ?? '' },
    { name: 'name', label: t('common.name'), required: true, defaultValue: selectedCase?.name ?? '' },
    { name: 'input', label: t('datasets.inputJson'), type: 'textarea', defaultValue: JSON.stringify(selectedCase?.input ?? {}, null, 2) },
    { name: 'expectedOutput', label: t('datasets.expectedJson'), type: 'textarea', defaultValue: JSON.stringify(selectedCase?.expectedOutput ?? {}, null, 2) },
    { name: 'context', label: t('datasets.contextJson'), type: 'textarea', defaultValue: selectedCase?.context ? JSON.stringify(selectedCase.context, null, 2) : '' },
    { name: 'tags', label: t('datasets.tags'), placeholder: t('datasets.tagsPlaceholder'), defaultValue: selectedCase?.tags.join(', ') ?? '' },
  ]
  return <PageContainer>
    <PageHeader action={auth.hasPermission('dataset:manage') ? <div className="flex gap-2"><Button onClick={() => setForm('dataset')} variant="secondary"><Edit3 className="size-4" />{t('common.edit')}</Button><input accept=".csv,.jsonl" className="hidden" onChange={(event) => { const file = event.target.files?.[0]; if (file) void importFile(file); event.target.value = '' }} ref={inputRef} type="file" /><Button onClick={() => inputRef.current?.click()} variant="secondary"><Upload className="size-4" />{t('datasets.import')}</Button><Button onClick={() => void exportFile()} variant="secondary"><Download className="size-4" />{t('datasets.export')}</Button><PrerequisiteAction description={t('datasets.prerequisites.publishDescription')} loading={cases.isLoading} onReady={() => publish.mutate()} requirements={[{ key: 'case', label: t('datasets.prerequisites.case'), met: Boolean(dataset.data.caseCount), actionLabel: t('datasets.prerequisites.addCase'), onAction: () => { setSelectedCase(null); setForm('case') } }]} disabled={publish.isPending}><FilePlus2 className="size-4" />{t('datasets.publishVersion')}</PrerequisiteAction></div> : undefined} description={dataset.data.description ?? t('datasets.description')} title={dataset.data.name} />
    <Tabs className="mt-6" defaultValue="cases"><TabsList className="border-b border-border"><TabsTrigger value="cases">{t('datasets.cases')} ({dataset.data.caseCount})</TabsTrigger><TabsTrigger value="versions">{t('datasets.versions')}</TabsTrigger></TabsList><TabsContent className="pt-5" value="cases"><Card className="overflow-hidden"><div className="flex h-14 items-center justify-between border-b border-border px-5"><span className="text-xs text-muted-foreground">{t('datasets.revision')} {dataset.data.revision}</span>{auth.hasPermission('dataset:manage') && <Button onClick={() => { setSelectedCase(null); setForm('case') }} size="sm"><Plus className="size-4" />{t('datasets.addCase')}</Button>}</div><DataTable columns={columns} data={cases.data ?? []} getSearchText={(item) => `${item.caseKey} ${item.name} ${item.tags.join(' ')}`} searchPlaceholder={t('datasets.search')} /></Card></TabsContent><TabsContent className="pt-5" value="versions"><Card className="divide-y divide-border">{versions.data?.map((version) => <div className="flex items-center px-5 py-4 text-xs" key={version.id}><strong>v{version.versionNumber}</strong><span className="ml-4 text-muted-foreground">{t('datasets.versionSummary', { count: version.caseCount, revision: version.sourceRevision })}</span><code className="ml-auto text-[10px] text-muted-foreground">{version.contentHash.slice(0, 20)}…</code></div>)}</Card></TabsContent></Tabs>
    {form && <EntityFormDialog cancelLabel={t('common.cancel')} fields={form === 'dataset' ? datasetFields : caseFields} onClose={() => setForm(null)} onSubmit={(values) => save.mutateAsync({ kind: form, values }).then(() => undefined)} open submitLabel={t('common.save')} title={form === 'dataset' ? `${t('common.edit')} ${t('datasets.title')}` : selectedCase ? t('datasets.editCase') : t('datasets.addCase')} />}
    <ConfirmDialog cancelLabel={t('common.cancel')} confirmLabel={t('common.delete')} description={t('datasets.confirmDelete')} onClose={() => setDeleteCase(null)} onConfirm={async () => { if (deleteCase) await remove.mutateAsync(deleteCase) }} open={Boolean(deleteCase)} pending={remove.isPending} title={t('datasets.deleteCase')} />
  </PageContainer>
}
