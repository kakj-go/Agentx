import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { ColumnDef } from '@tanstack/react-table'
import { Download, PackageOpen, Upload } from 'lucide-react'
import { useMemo, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, apiRequestBlob, jsonBody } from '../../shared/api/client'
import type { PageResponse } from '../../shared/api/types'
import { EntityCell } from '../../shared/components/entity-cell'
import { ListPage } from '../../shared/components/list-page'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { Dialog, DialogContent } from '../../shared/ui/dialog'
import { useToast } from '../../shared/ui/toast'
import type { CanvasPlugin, CanvasPluginImport } from './types'

export function CanvasPluginsPage() {
  const { t } = useTranslation()
  const auth = useAuth()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const input = useRef<HTMLInputElement>(null)
  const [open, setOpen] = useState(false)
  const [preview, setPreview] = useState<CanvasPluginImport>()
  const [enableVersion, setEnableVersion] = useState(true)
  const [setDefaultVersion, setSetDefaultVersion] = useState(true)
  const plugins = useQuery({ queryKey: ['canvas-plugins'], queryFn: () => apiRequest<PageResponse<CanvasPlugin>>('/canvas-plugins?pageSize=100') })
  const upload = useMutation({
    mutationFn: async (file: File) => { const body = new FormData(); body.append('file', file); return apiRequest<CanvasPluginImport>('/canvas-plugin-imports', { method: 'POST', body }) },
    onSuccess: setPreview, onError: (error: Error) => showToast(error.message),
  })
  const install = useMutation({
    mutationFn: () => apiRequest<CanvasPlugin>(`/canvas-plugin-imports/${preview?.id}/install`, { method: 'POST', body: jsonBody({ bundleDigest: preview?.bundleDigest, enable: enableVersion, setDefault: enableVersion && setDefaultVersion }) }),
    onSuccess: async () => { setOpen(false); setPreview(undefined); await queryClient.invalidateQueries({ queryKey: ['canvas-plugins'] }); await queryClient.invalidateQueries({ queryKey: ['node-definitions'] }); showToast(t('canvasPlugins.imported')) },
    onError: (error: Error) => showToast(error.message),
  })
  const downloadTemplate = async () => {
    const blob = await apiRequestBlob('/canvas-plugin-sdk/template'); const url = URL.createObjectURL(blob)
    const anchor = document.createElement('a'); anchor.href = url; anchor.download = 'agentx-canvas-plugin-template.zip'; anchor.click(); URL.revokeObjectURL(url)
  }
  const columns = useMemo<Array<ColumnDef<CanvasPlugin>>>(() => [
    { accessorKey: 'displayName', header: t('canvasPlugins.fields.name'), cell: ({ row }) => <EntityCell detail={row.original.packageId} icon={PackageOpen} name={row.original.displayName} /> },
    { accessorKey: 'sourceType', header: t('canvasPlugins.fields.status'), cell: ({ row }) => <StatusBadge status={row.original.versions.some((version) => version.status === 'enabled') ? 'active' : 'inactive'} /> },
    { accessorKey: 'sourceType', id: 'source', header: t('canvasPlugins.fields.source'), cell: ({ row }) => t(`canvasPlugins.sources.${row.original.sourceType}`) },
    { accessorKey: 'versions', header: t('canvasPlugins.fields.versions'), cell: ({ row }) => row.original.versions.length },
    { accessorKey: 'nodeCount', header: t('canvasPlugins.fields.nodes') },
    { id: 'actions', header: '', cell: ({ row }) => <Button asChild size="sm" variant="ghost"><Link to={`/canvas-plugins/${row.original.id}`}>{t('canvasPlugins.details')}</Link></Button> },
  ], [t])
  const actions = <div className="flex gap-2">
    <Button onClick={() => void downloadTemplate().catch((error: Error) => showToast(error.message))} variant="secondary"><Download className="size-4" />{t('canvasPlugins.template')}</Button>
    {auth.hasPermission('canvas_plugin:manage') && <Button onClick={() => setOpen(true)}><Upload className="size-4" />{t('canvasPlugins.import')}</Button>}
  </div>
  return <>
    <ListPage action={actions} categoryLabel={t('canvasPlugins.fields.source')} categoryOptions={[{ value: 'builtin', label: t('canvasPlugins.sources.builtin') }, { value: 'imported', label: t('canvasPlugins.sources.imported') }]} columns={columns} data={plugins.data?.items ?? []} description={t('canvasPlugins.description')} getCategory={(value) => value.sourceType} getSearchText={(value) => `${value.displayName} ${value.packageId} ${value.description}`} getStatus={(value) => value.versions.some((version) => version.status === 'enabled') ? 'enabled' : 'disabled'} searchPlaceholder={t('canvasPlugins.search')} statusOptions={[{ value: 'enabled', label: t('canvasPlugins.enabled') }, { value: 'disabled', label: t('canvasPlugins.disabled') }]} title={t('canvasPlugins.title')} />
    <Dialog onOpenChange={(value) => { setOpen(value); if (!value) setPreview(undefined); setEnableVersion(true); setSetDefaultVersion(true) }} open={open}><DialogContent title={t('canvasPlugins.importTitle')}>
      <div className="space-y-5 p-5"><div><h2 className="text-sm font-semibold">{t('canvasPlugins.importTitle')}</h2><p className="mt-1 text-xs text-muted-foreground">{t('canvasPlugins.description')}</p></div>
        <input accept=".agentx-plugin,application/zip" className="hidden" onChange={(event) => { const file = event.target.files?.[0]; if (file) upload.mutate(file); event.target.value = '' }} ref={input} type="file" />
        {!preview ? <button className="grid min-h-36 w-full place-items-center rounded-lg border border-dashed border-border bg-canvas/40 text-xs text-muted-foreground hover:border-primary" onClick={() => input.current?.click()} type="button"><span><PackageOpen className="mx-auto mb-3 size-8 text-primary" />{upload.isPending ? t('canvasPlugins.validating') : t('canvasPlugins.chooseFile')}</span></button> : <div className="rounded-lg border border-border p-4"><strong className="text-sm">{preview.displayName}</strong><p className="mt-1 font-mono text-[10px] text-muted-foreground">{preview.packageId}@{preview.packageVersion}</p><p className="mt-3 text-xs text-muted-foreground">{preview.description}</p><div className="mt-3 flex flex-wrap gap-2">{preview.nodeTypes.map((node) => <span className="rounded bg-muted px-2 py-1 font-mono text-[10px]" key={node}>{node}</span>)}</div><p className="mt-3 break-all font-mono text-[9px] text-muted-foreground">{preview.bundleDigest}</p>{preview.status === 'failed' && <div className="mt-3 rounded bg-danger/5 p-2 text-[10px] text-danger">{preview.issues.map((issue) => JSON.stringify(issue)).join('\n')}</div>}</div>}
        {preview?.status === 'ready' && <div className="grid gap-2 rounded-md border border-border p-3 text-xs"><label className="flex items-center gap-2"><input checked={enableVersion} onChange={(event) => { setEnableVersion(event.target.checked); if (!event.target.checked) setSetDefaultVersion(false) }} type="checkbox" />{t('canvasPlugins.enableAfterImport')}</label><label className="flex items-center gap-2"><input checked={setDefaultVersion} disabled={!enableVersion} onChange={(event) => setSetDefaultVersion(event.target.checked)} type="checkbox" />{t('canvasPlugins.defaultAfterImport')}</label></div>}
        <div className="flex justify-end gap-2"><Button onClick={() => setOpen(false)} variant="ghost">{t('common.cancel')}</Button><Button disabled={preview?.status !== 'ready' || install.isPending} onClick={() => install.mutate()}>{t('canvasPlugins.confirmImport')}</Button></div>
      </div>
    </DialogContent></Dialog>
  </>
}
