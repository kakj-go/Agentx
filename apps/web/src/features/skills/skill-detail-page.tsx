import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ArchiveRestore, Download, File, FilePlus2, FileText, Folder, FolderPlus, Image, Move, Pencil, Power, Save, Trash2, Upload } from 'lucide-react'
import { useEffect, useMemo, useRef, useState, type DragEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { useParams } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, apiRequestBlob, apiRequestText, jsonBody } from '../../shared/api/client'
import type { Skill, SkillVersion, SkillWorkspace, SkillWorkspaceEntry } from '../../shared/api/types'
import { EntityFormDialog, type EntityFormField } from '../../shared/components/entity-form-dialog'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { PrerequisiteAction } from '../../shared/components/prerequisite-action'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { Dialog, DialogContent } from '../../shared/ui/dialog'
import { MarkdownEditor, type MarkdownEditorHandle } from '../../shared/ui/markdown-editor'
import { Textarea } from '../../shared/ui/textarea'
import { useToast } from '../../shared/ui/toast'
import { parseSkillMarkdown } from './skill-markdown'
import { skillReferenceMarkdown, WORKSPACE_ROOT_PARENT, workspaceParentId, workspaceParentValue } from './skill-workspace'

type DialogKind = 'create' | 'move' | 'reference' | 'publish' | 'edit' | undefined

export function SkillDetailPage() {
  const { id = '' } = useParams()
  const { t } = useTranslation()
  const auth = useAuth()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const fileInput = useRef<HTMLInputElement>(null)
  const importInput = useRef<HTMLInputElement>(null)
  const markdownEditor = useRef<MarkdownEditorHandle>(null)
  const [dialog, setDialog] = useState<DialogKind>()
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [createKind, setCreateKind] = useState<'directory' | 'file'>('file')
  const [selectedId, setSelectedId] = useState('')
  const [content, setContent] = useState('')
  const [description, setDescription] = useState('')
  const [dirty, setDirty] = useState(false)

  const skill = useQuery({ queryKey: ['skill', id], queryFn: () => apiRequest<Skill>(`/skills/${id}`) })
  const workspace = useQuery({ queryKey: ['skill-workspace', id], queryFn: () => apiRequest<SkillWorkspace>(`/skills/${id}/workspace`) })
  const versions = useQuery({ queryKey: ['skill-versions', id], queryFn: () => apiRequest<SkillVersion[]>(`/skills/${id}/versions`) })
  const selected = workspace.data?.entries.find((entry) => entry.id === selectedId)
  const fileContent = useQuery({ queryKey: ['skill-file', id, selectedId, selected?.contentHash], enabled: Boolean(selected?.editable), queryFn: () => apiRequestText(`/skills/${id}/files/${selectedId}`) })
  useEffect(() => {
    if (!selectedId && workspace.data?.entries.length) setSelectedId(workspace.data.entries.find((entry) => entry.path === 'SKILL.md')?.id ?? workspace.data.entries[0].id)
  }, [selectedId, workspace.data])
  useEffect(() => {
    if (fileContent.data === undefined) return
    const document = selected?.path === 'SKILL.md' ? parseSkillMarkdown(fileContent.data, skill.data?.description ?? '') : { body: fileContent.data, description: '' }
    setContent(document.body)
    if (selected?.path === 'SKILL.md') setDescription(document.description)
    setDirty(false)
  }, [fileContent.data, selected?.path, skill.data?.description])

  const directories = useMemo(() => workspace.data?.entries.filter((entry) => entry.entryType === 'directory') ?? [], [workspace.data])
  const files = useMemo(() => workspace.data?.entries.filter((entry) => entry.entryType === 'file' && entry.id !== selectedId) ?? [], [selectedId, workspace.data])
  const refresh = async () => Promise.all([
    queryClient.invalidateQueries({ queryKey: ['skill', id] }), queryClient.invalidateQueries({ queryKey: ['skills'] }),
    queryClient.invalidateQueries({ queryKey: ['skill-workspace', id] }), queryClient.invalidateQueries({ queryKey: ['skill-file', id] }),
    queryClient.invalidateQueries({ queryKey: ['skill-versions', id] }),
  ])
  const createEntry = async (values: Record<string, string>) => {
    await apiRequest(`/skills/${id}/entries`, { method: 'POST', body: jsonBody({ parentId: workspaceParentId(values.parent), name: values.name, entryType: createKind, expectedRevision: workspace.data?.revision }) })
    await refresh()
  }
  const moveEntry = async (values: Record<string, string>) => {
    if (!selected) return
    await apiRequest(`/skills/${id}/entries/${selected.id}`, { method: 'PATCH', body: jsonBody({ parentId: workspaceParentId(values.parent), name: values.name, expectedRevision: workspace.data?.revision }) })
    await refresh()
    showToast(t('skills.referencesUpdated'))
  }
  const remove = useMutation({
    mutationFn: () => apiRequest(`/skills/${id}/entries/${selectedId}?expectedRevision=${workspace.data?.revision}`, { method: 'DELETE' }),
    onSuccess: async () => { setSelectedId(''); setDeleteOpen(false); await refresh() },
    onError: (error: Error) => showToast(error.message),
  })
  const save = useMutation({
    mutationFn: () => apiRequest(`/skills/${id}/files/${selectedId}`, {
      method: 'PUT',
      body: jsonBody({
        content,
        description: selected?.path === 'SKILL.md' ? description : undefined,
        expectedRevision: workspace.data?.revision,
      }),
    }),
    onSuccess: async () => { setDirty(false); await refresh(); showToast(t('skills.saved')) },
    onError: (error: Error) => showToast(error.message),
  })
  const upload = async (file: globalThis.File) => {
    const body = new FormData()
    body.append('parentId', selected?.entryType === 'directory' ? selected.id : selected?.parentId ?? '')
    body.append('expectedRevision', String(workspace.data?.revision ?? 0))
    body.append('file', file)
    await apiRequest(`/skills/${id}/uploads`, { method: 'POST', body })
    await refresh()
    showToast(t('skills.uploaded'))
  }
  const importWorkspace = async (file: globalThis.File) => {
    const body = new FormData()
    body.append('expectedRevision', String(workspace.data?.revision ?? 0))
    body.append('file', file)
    await apiRequest(`/skills/${id}/workspace/import`, { method: 'POST', body })
    setSelectedId('')
    await refresh()
    showToast(t('skills.workspace.imported'))
  }
  const exportWorkspace = async () => {
    const blob = await apiRequestBlob(`/skills/${id}/workspace/export`)
    const url = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = url; anchor.download = `${skill.data?.name ?? 'skill'}-workspace.zip`; anchor.click()
    URL.revokeObjectURL(url)
  }
  const publish = async (values: Record<string, string>) => {
    let dependencies: unknown
    try { dependencies = JSON.parse(values.dependencies || '[]') as unknown } catch { throw new Error(t('skills.invalidJson')) }
    await apiRequest(`/skills/${id}/versions`, { method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() }, body: jsonBody({ expectedRevision: workspace.data?.revision, dependencies }) })
    await refresh()
    showToast(t('skills.publishedWorkspace'))
  }
  const editSkill = async (values: Record<string, string>) => {
    await apiRequest(`/skills/${id}`, { method: 'PATCH', body: jsonBody({ name: values.name, alias: values.alias, description: skill.data?.description ?? null, status: skill.data?.status, version: skill.data?.version }) })
    await refresh()
  }
  const toggle = useMutation({
    mutationFn: () => apiRequest(`/skills/${id}`, { method: 'PATCH', body: jsonBody({ name: skill.data?.name, alias: skill.data?.alias, description: skill.data?.description ?? null, status: skill.data?.status === 'active' ? 'disabled' : 'active', version: skill.data?.version }) }),
    onSuccess: refresh,
    onError: (error: Error) => showToast(error.message),
  })
  const insertReference = (values: Record<string, string>) => {
    const target = files.find((entry) => entry.id === values.file)
    if (!selected || !target) return Promise.resolve()
    markdownEditor.current?.insertText(skillReferenceMarkdown(selected.path, target.path, target.name, target.mimeType))
    return Promise.resolve()
  }
  const onDrop = (event: DragEvent<HTMLDivElement>) => { event.preventDefault(); const file = event.dataTransfer.files[0]; if (file) void upload(file).catch((error: Error) => showToast(error.message)) }
  const actions = auth.hasPermission('skill:manage') && skill.data ? <div className="flex gap-2">
    <input accept=".zip,application/zip" className="hidden" onChange={(event) => { const file = event.target.files?.[0]; if (file) void importWorkspace(file).catch((error: Error) => showToast(error.message)); event.target.value = '' }} ref={importInput} type="file" />
    <Button onClick={() => setDialog('edit')} variant="secondary"><Pencil className="size-4" />{t('common.edit')}</Button>
    <Button onClick={() => importInput.current?.click()} variant="secondary"><ArchiveRestore className="size-4" />{t('skills.workspace.import')}</Button>
    <Button onClick={() => void exportWorkspace().catch((error: Error) => showToast(error.message))} variant="secondary"><Download className="size-4" />{t('skills.workspace.export')}</Button>
    <PrerequisiteAction description={t('skills.prerequisites.enableDescription')} disabled={toggle.isPending} loading={versions.isLoading} onReady={() => toggle.mutate()} requirements={[{ key: 'version', label: t('skills.prerequisites.version'), met: Boolean(skill.data.latestVersion), actionLabel: t('skills.prerequisites.publish'), onAction: () => setDialog('publish') }]} variant="secondary"><Power className="size-4" />{skill.data.status === 'active' ? t('skills.disable') : t('skills.enable')}</PrerequisiteAction>
    <Button onClick={() => setDialog('publish')}><Save className="size-4" />{t('skills.publishWorkspace')}</Button>
  </div> : undefined

  if (skill.isLoading || workspace.isLoading) return <PageContainer><p className="text-sm text-muted-foreground">{t('skills.loading')}</p></PageContainer>
  if (skill.error || workspace.error || !skill.data || !workspace.data) return <PageContainer><p className="text-sm text-danger">{String(skill.error ?? workspace.error ?? t('skills.loadFailed'))}</p></PageContainer>
  return <PageContainer className="max-w-none">
    <PageHeader action={actions} description={skill.data.description ?? t('skills.description')} title={skill.data.name} />
    <div className="mt-3 flex items-center gap-3 text-xs text-muted-foreground"><StatusBadge status={skill.data.status === 'active' ? 'active' : skill.data.status === 'draft' ? 'draft' : 'inactive'} /><span>{t('skills.fields.alias')}: {skill.data.alias}</span><span>{t('skills.workspaceRevision', { revision: workspace.data.revision })}</span><span>{workspace.data.entries.length} {t('skills.entries')}</span></div>
    <div className="mt-5 grid h-[calc(100vh-250px)] min-h-[560px] grid-cols-[260px_minmax(0,1fr)_300px] overflow-hidden rounded-lg border border-border bg-surface">
      <aside className="flex min-h-0 flex-col border-r border-border">
        <div className="flex h-12 items-center gap-1 border-b border-border px-3"><span className="text-xs font-semibold">{t('skills.files')}</span><div className="flex-1" />{auth.hasPermission('skill:manage') && <><Button aria-label={t('skills.newFolder')} onClick={() => { setCreateKind('directory'); setDialog('create') }} size="icon" variant="ghost"><FolderPlus className="size-4" /></Button><Button aria-label={t('skills.newMarkdown')} onClick={() => { setCreateKind('file'); setDialog('create') }} size="icon" variant="ghost"><FilePlus2 className="size-4" /></Button><Button aria-label={t('skills.uploadFile')} onClick={() => fileInput.current?.click()} size="icon" variant="ghost"><Upload className="size-4" /></Button></>}</div>
        <input className="hidden" onChange={(event) => { const file = event.target.files?.[0]; if (file) void upload(file).catch((error: Error) => showToast(error.message)); event.target.value = '' }} ref={fileInput} type="file" />
        <div className="min-h-0 flex-1 overflow-y-auto p-2" onDragOver={(event) => event.preventDefault()} onDrop={onDrop}><FileTree entries={workspace.data.entries} onSelect={setSelectedId} selectedId={selectedId} /></div>
        <div className="border-t border-border p-3 text-[10px] leading-4 text-muted-foreground">{t('skills.dropUpload')}</div>
      </aside>
      <main className="flex min-h-0 min-w-0 flex-col">
        <div className="flex h-12 items-center gap-2 border-b border-border px-4"><FileText className="size-4 text-primary" /><span className="truncate text-xs font-semibold">{selected?.path ?? t('skills.selectFile')}</span><div className="flex-1" />{selected && selected.path !== 'SKILL.md' && auth.hasPermission('skill:manage') && <><Button onClick={() => setDialog('move')} size="sm" variant="ghost"><Move className="size-3.5" />{t('skills.moveRename')}</Button><Button aria-label={t('skills.deleteEntry')} onClick={() => setDeleteOpen(true)} size="icon" variant="ghost"><Trash2 className="size-3.5" /></Button></>}{selected?.editable && auth.hasPermission('skill:manage') && <Button disabled={!dirty || save.isPending || (selected.path === 'SKILL.md' && !description.trim())} onClick={() => save.mutate()} size="sm"><Save className="size-3.5" />{t('common.save')}</Button>}</div>
        <div className="flex min-h-0 flex-1 flex-col">
          {selected?.path === 'SKILL.md' && selected.editable && <div className="shrink-0 border-b border-border bg-canvas/30 px-5 py-3">
            <label className="text-xs font-medium text-foreground" htmlFor="skill-description">{t('skills.fields.description')}</label>
            <Textarea aria-describedby="skill-description-help skill-description-count" aria-required="true" className="mt-2 min-h-20 resize-y" id="skill-description" maxLength={1000} onChange={(event) => { setDescription(event.target.value); setDirty(true) }} placeholder={t('skills.fields.descriptionPlaceholder')} value={description} />
            <div className="mt-1.5 flex items-start justify-between gap-4 text-[10px] text-muted-foreground"><span id="skill-description-help">{t('skills.fields.descriptionHint')}</span><span className="shrink-0 tabular-nums" id="skill-description-count">{t('skills.fields.characterCount', { count: description.length, max: 1000 })}</span></div>
          </div>}
          <div className="min-h-0 flex-1">{selected?.editable ? <MarkdownEditor onChange={(value) => { setContent(value); setDirty(true) }} ref={markdownEditor} toolbarAction={auth.hasPermission('skill:manage') ? { disabled: files.length === 0, label: t('skills.insertReference'), onClick: () => setDialog('reference') } : undefined} value={content} /> : selected?.entryType === 'file' ? <FilePreview entry={selected} skillId={id} /> : <div className="grid h-full place-items-center text-xs text-muted-foreground">{t('skills.directorySelected')}</div>}</div>
        </div>
      </main>
      <aside className="min-h-0 overflow-y-auto border-l border-border bg-canvas/40 p-4">
        <h2 className="text-xs font-semibold">{t('skills.fileDetails')}</h2>{selected ? <dl className="mt-4 space-y-3 text-xs"><Detail label={t('skills.path')} value={selected.path} /><Detail label={t('skills.entryType')} value={selected.entryType} /><Detail label={t('skills.mimeType')} value={selected.mimeType ?? '—'} /><Detail label={t('skills.fileSize')} value={`${selected.sizeBytes} B`} /><Detail label="SHA-256" value={selected.contentHash?.slice(0, 24) ?? '—'} /></dl> : <p className="mt-4 text-xs text-muted-foreground">{t('skills.selectFile')}</p>}
        <div className="mt-7 border-t border-border pt-5"><h2 className="text-xs font-semibold">{t('skills.versions')}</h2><div className="mt-3 space-y-2">{versions.data?.map((version) => <div className="rounded-md border border-border bg-surface p-3 text-xs" key={version.id}><p className="font-medium">v{version.versionNumber}</p><p className="mt-1 text-[10px] text-muted-foreground">r{version.sourceRevision} · {version.fileCount} {t('skills.files')}</p><p className="mt-1 truncate font-mono text-[9px] text-muted-foreground">{version.contentHash}</p></div>)}{versions.data?.length === 0 && <p className="text-xs text-muted-foreground">{t('skills.noVersion')}</p>}</div></div>
      </aside>
    </div>
    {dialog === 'create' && <EntityFormDialog cancelLabel={t('common.cancel')} fields={[{ name: 'name', label: createKind === 'file' ? t('skills.markdownName') : t('skills.folderName'), required: true, placeholder: createKind === 'file' ? 'guide.md' : undefined }, parentField(directories, t)]} onClose={() => setDialog(undefined)} onSubmit={createEntry} open submitLabel={t('common.save')} title={createKind === 'file' ? t('skills.newMarkdown') : t('skills.newFolder')} />}
    {dialog === 'move' && selected && <EntityFormDialog cancelLabel={t('common.cancel')} fields={[{ name: 'name', label: t('common.name'), defaultValue: selected.name, required: true }, parentField(directories.filter((entry) => !entry.path.startsWith(`${selected.path}/`) && entry.id !== selected.id), t, selected.parentId)]} onClose={() => setDialog(undefined)} onSubmit={moveEntry} open submitLabel={t('common.save')} title={t('skills.moveRename')} />}
    {dialog === 'reference' && <EntityFormDialog cancelLabel={t('common.cancel')} fields={[{ name: 'file', label: t('skills.referenceTarget'), type: 'select', required: true, options: files.map((entry) => ({ value: entry.id, label: entry.path })) }]} onClose={() => setDialog(undefined)} onSubmit={insertReference} open submitLabel={t('skills.insertReference')} title={t('skills.insertReference')} />}
    {dialog === 'publish' && <EntityFormDialog cancelLabel={t('common.cancel')} fields={[{ name: 'dependencies', label: t('skills.dependencies'), type: 'textarea', defaultValue: '[]' }]} onClose={() => setDialog(undefined)} onSubmit={publish} open submitLabel={t('skills.publish')} title={t('skills.publishWorkspace')} />}
    {dialog === 'edit' && <EntityFormDialog cancelLabel={t('common.cancel')} fields={[{ name: 'name', label: t('skills.fields.name'), defaultValue: skill.data.name, maxLength: 160, required: true }, { name: 'alias', label: t('skills.fields.alias'), defaultValue: skill.data.alias, description: t('skills.fields.aliasHint'), maxLength: 160, required: true }]} onClose={() => setDialog(undefined)} onSubmit={editSkill} open submitLabel={t('common.save')} title={t('skills.editSkill')} />}
    <Dialog onOpenChange={setDeleteOpen} open={deleteOpen}><DialogContent title={t('skills.deleteEntry')}><div className="p-5"><h2 className="text-sm font-semibold">{t('skills.deleteEntry')}</h2><p className="mt-2 text-xs text-muted-foreground">{t('skills.deleteEntryConfirm', { path: selected?.path })}</p><div className="mt-5 flex justify-end gap-2"><Button onClick={() => setDeleteOpen(false)} variant="ghost">{t('common.cancel')}</Button><Button disabled={remove.isPending} onClick={() => remove.mutate()} variant="danger">{t('skills.deleteEntry')}</Button></div></div></DialogContent></Dialog>
  </PageContainer>
}

function FileTree({ entries, onSelect, selectedId }: { entries: SkillWorkspaceEntry[]; onSelect: (id: string) => void; selectedId: string }) {
  const children = (parent?: string | null) => entries.filter((entry) => (entry.parentId ?? null) === (parent ?? null)).sort((left, right) => left.entryType === right.entryType ? left.name.localeCompare(right.name) : left.entryType === 'directory' ? -1 : 1)
  const render = (parent: string | null, depth: number): React.ReactNode => children(parent).map((entry) => {
    const Icon = entry.entryType === 'directory' ? Folder : entry.mimeType?.startsWith('image/') ? Image : entry.editable ? FileText : File
    return <div key={entry.id}><button className={`flex h-8 w-full items-center gap-2 rounded-md pr-2 text-left text-xs ${entry.id === selectedId ? 'bg-primary/10 text-primary' : 'text-foreground hover:bg-muted'}`} onClick={() => onSelect(entry.id)} style={{ paddingLeft: `${8 + depth * 14}px` }} type="button"><Icon className="size-3.5 shrink-0" /><span className="truncate">{entry.name}</span></button>{entry.entryType === 'directory' && render(entry.id, depth + 1)}</div>
  })
  return render(null, 0)
}

function FilePreview({ entry, skillId }: { entry: SkillWorkspaceEntry; skillId: string }) {
  const { t } = useTranslation()
  const file = useQuery({ queryKey: ['skill-preview', skillId, entry.id, entry.contentHash], queryFn: () => apiRequestBlob(`/skills/${skillId}/files/${entry.id}`) })
  const [url, setUrl] = useState('')
  const [text, setText] = useState('')
  useEffect(() => {
    if (!file.data) return
    if (entry.mimeType?.startsWith('text/') || isCode(entry.name)) void file.data.text().then(setText)
    const next = URL.createObjectURL(file.data); setUrl(next)
    return () => URL.revokeObjectURL(next)
  }, [entry.mimeType, entry.name, file.data])
  if (file.isLoading) return <div className="grid h-full place-items-center text-xs text-muted-foreground">{t('skills.loading')}</div>
  if (entry.mimeType?.startsWith('image/')) return <div className="grid h-full place-items-center overflow-auto bg-canvas p-6"><img alt={entry.name} className="max-h-full max-w-full object-contain" src={url} /></div>
  if (entry.mimeType === 'application/pdf') return <iframe className="h-full w-full border-0" src={url} title={entry.name} />
  if (entry.mimeType?.startsWith('text/') || isCode(entry.name)) return <pre className="h-full overflow-auto bg-canvas p-5 text-xs leading-5">{text}</pre>
  return <div className="grid h-full place-items-center"><div className="text-center text-xs text-muted-foreground"><File className="mx-auto mb-3 size-8" /><p>{t('skills.binaryPreview')}</p><p className="mt-1">{entry.mimeType ?? 'application/octet-stream'} · {entry.sizeBytes} B</p></div></div>
}

function Detail({ label, value }: { label: string; value: string }) { return <div><dt className="text-[10px] text-muted-foreground">{label}</dt><dd className="mt-1 break-all font-mono text-[11px]">{value}</dd></div> }
function parentField(entries: SkillWorkspaceEntry[], t: (key: string) => string, current?: string | null): EntityFormField { return { name: 'parent', label: t('skills.parentFolder'), type: 'select', defaultValue: workspaceParentValue(current), options: [{ value: WORKSPACE_ROOT_PARENT, label: t('skills.workspaceRoot') }, ...entries.map((entry) => ({ value: entry.id, label: entry.path }))] } }
function isCode(name: string) { return /\.(json|ya?ml|toml|rs|ts|tsx|js|jsx|py|sh|sql|css|html|xml)$/i.test(name) }
