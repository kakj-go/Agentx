import { ArrowRight, Check, FileEdit, GitCommitHorizontal, LoaderCircle, Rocket, X } from 'lucide-react'
import { useEffect, useMemo, useState, type Dispatch, type SetStateAction } from 'react'
import { useTranslation } from 'react-i18next'

import type { WorkflowDeployment, WorkflowEnvironment, WorkflowVersion } from '../../shared/api/types'
import { Button } from '../../shared/ui/button'
import { Dialog, DialogContent } from '../../shared/ui/dialog'
import { Select } from '../../shared/ui/select'

export type WorkflowPublishInput = { environmentId: string; workflowVersionId?: string; useDraft: boolean }
export type WorkflowPublishStage = 'idle' | 'creating' | 'publishing'

type Props = {
  open: boolean
  draftRevision: number
  dirty: boolean
  versions: WorkflowVersion[]
  environments: WorkflowEnvironment[]
  deployments: WorkflowDeployment[]
  defaultEnvironmentId?: string
  defaultVersionId?: string
  onClose: () => void
  onSubmit: (input: WorkflowPublishInput, setStage: Dispatch<SetStateAction<WorkflowPublishStage>>) => Promise<void>
}

const CURRENT_DRAFT = '__current_draft__'

export function WorkflowPublishDialog({ open, draftRevision, dirty, versions, environments, deployments, defaultEnvironmentId, defaultVersionId, onClose, onSubmit }: Props) {
  const { t } = useTranslation()
  const [environmentId, setEnvironmentId] = useState('')
  const [versionId, setVersionId] = useState('')
  const [stage, setStage] = useState<WorkflowPublishStage>('idle')
  const [error, setError] = useState('')
  const latest = versions[0]
  const nextVersion = (latest?.versionNumber ?? 0) + 1
  const pending = stage !== 'idle'

  useEffect(() => {
    if (!open) return
    setEnvironmentId(defaultEnvironmentId && environments.some((item) => item.id === defaultEnvironmentId) ? defaultEnvironmentId : environments[0]?.id ?? '')
    setVersionId(defaultVersionId && versions.some((item) => item.id === defaultVersionId) ? defaultVersionId : dirty ? CURRENT_DRAFT : latest?.id ?? '')
    setStage('idle')
    setError('')
  }, [defaultEnvironmentId, defaultVersionId, dirty, environments, latest?.id, open, versions])

  const environment = environments.find((item) => item.id === environmentId)
  const version = versions.find((item) => item.id === versionId)
  const useDraft = versionId === CURRENT_DRAFT
  const targetVersionNumber = useDraft ? nextVersion : version?.versionNumber
  const active = deployments.find((item) => item.environmentId === environmentId && item.status === 'active')
  const noOp = Boolean(version && active?.workflowVersionId === version.id)
  const environmentOptions = useMemo(() => environments.map((item) => ({ value: item.id, label: item.name })), [environments])
  const versionOptions = useMemo(() => [
    ...(dirty ? [{ value: CURRENT_DRAFT, label: t('workflows.publishDialog.currentDraftOption', { revision: draftRevision, version: nextVersion }) }] : []),
    ...versions.map((item) => ({ value: item.id, label: t('workflows.publishDialog.existingVersionOption', { version: item.versionNumber }) })),
  ], [dirty, draftRevision, nextVersion, t, versions])

  const submit = async () => {
    if (!environmentId || (!useDraft && !versionId) || noOp) return
    setError('')
    setStage(useDraft ? 'creating' : 'publishing')
    try {
      await onSubmit({ environmentId, workflowVersionId: useDraft ? undefined : versionId, useDraft }, setStage)
      onClose()
    } catch (value) {
      setStage('idle')
      setError(value instanceof Error ? value.message : String(value))
    }
  }

  const close = () => { if (!pending) onClose() }
  return <Dialog onOpenChange={(value) => !value && close()} open={open}><DialogContent description={t('workflows.publishDialog.description')} title={t('workflows.publishToEnvironment')}>
    <div className="flex items-start gap-4 border-b border-border px-6 py-5"><div><h2 className="text-lg font-semibold">{t('workflows.publishToEnvironment')}</h2><p className="mt-1 text-xs leading-5 text-muted-foreground">{t('workflows.publishDialog.description')}</p></div><Button aria-label={t('common.close')} className="ml-auto" disabled={pending} onClick={close} size="icon" variant="ghost"><X className="size-4" /></Button></div>
    <div className="space-y-5 px-6 py-5">
      <label className="block text-xs"><span className="mb-2 block font-medium">{t('workflows.publishDialog.targetEnvironment')}</span><Select aria-label={t('workflows.publishDialog.targetEnvironment')} className="w-full" disabled={pending} onValueChange={setEnvironmentId} options={environmentOptions} value={environmentId} /></label>
      <label className="block text-xs"><span className="mb-2 block font-medium">{t('workflows.publishDialog.releaseContent')}</span><Select aria-label={t('workflows.publishDialog.releaseContent')} className="w-full" disabled={pending} onValueChange={setVersionId} options={versionOptions} value={versionId} /></label>

      <div className={`rounded-lg border p-3 text-xs leading-5 ${noOp ? 'border-warning/20 bg-warning/10 text-warning' : 'border-primary/15 bg-primary/5 text-foreground'}`}>
        {noOp ? t('workflows.publishDialog.noOp', { environment: environment?.name, version: targetVersionNumber }) : useDraft ? t('workflows.publishDialog.combinedDescription', { revision: draftRevision, version: targetVersionNumber, environment: environment?.name }) : t('workflows.publishDialog.existingDescription', { version: targetVersionNumber, environment: environment?.name })}
      </div>

      <div className="rounded-lg border border-border p-4"><p className="mb-3 text-[10px] text-muted-foreground">{t('workflows.publishDialog.changeSummary')}</p><div className="grid items-center gap-2 sm:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)_auto_minmax(0,1fr)]">
        <PreviewNode icon={useDraft ? FileEdit : GitCommitHorizontal} label={useDraft ? t('workflows.publishDialog.draftNode', { revision: draftRevision }) : t('workflows.publishDialog.versionNode', { version: targetVersionNumber })} />
        <ArrowRight className="mx-auto size-3.5 rotate-90 text-muted-foreground sm:rotate-0" />
        <PreviewNode icon={GitCommitHorizontal} label={t('workflows.publishDialog.immutableNode', { version: targetVersionNumber })} />
        <ArrowRight className="mx-auto size-3.5 rotate-90 text-muted-foreground sm:rotate-0" />
        <PreviewNode icon={Rocket} label={t('workflows.publishDialog.environmentNode', { environment: environment?.name ?? '—', from: active ? `v${active.versionNumber}` : t('workflows.publishDialog.notDeployed'), to: targetVersionNumber })} />
      </div></div>

      {pending && <div className="space-y-2 rounded-lg border border-border bg-muted/25 p-4"><ProgressRow active={stage === 'creating'} done={stage === 'publishing'} label={t('workflows.publishDialog.createVersionStep', { version: targetVersionNumber })} /><ProgressRow active={stage === 'publishing'} done={false} label={t('workflows.publishDialog.publishStep', { environment: environment?.name })} /></div>}
      {error && <p className="rounded-lg border border-danger/20 bg-danger/10 p-3 text-xs text-danger" role="alert">{error}</p>}
    </div>
    <div className="flex justify-end gap-2 border-t border-border bg-muted/30 px-6 py-4"><Button disabled={pending} onClick={close} variant="ghost">{t('common.cancel')}</Button><Button disabled={pending || noOp || !environmentId || !versionId} onClick={() => void submit()}><Rocket className="size-4" />{pending ? stage === 'creating' ? t('workflows.publishDialog.creatingVersion') : t('workflows.publishDialog.publishing') : t('workflows.publishToEnvironment')}</Button></div>
  </DialogContent></Dialog>
}

function PreviewNode({ icon: Icon, label }: { icon: typeof Rocket; label: string }) {
  return <div className="flex min-h-14 items-center justify-center gap-2 rounded-lg bg-muted/50 px-3 text-center text-[10px] font-medium"><Icon className="size-3.5 shrink-0 text-primary" />{label}</div>
}

function ProgressRow({ active, done, label }: { active: boolean; done: boolean; label: string }) {
  return <div className={`flex items-center gap-3 text-xs ${active ? 'text-primary' : done ? 'text-success' : 'text-muted-foreground'}`}><span className={`grid size-6 place-items-center rounded-full ${active ? 'bg-primary/10' : done ? 'bg-success/10' : 'bg-muted'}`}>{active ? <LoaderCircle className="size-3.5 animate-spin" /> : done ? <Check className="size-3.5" /> : <span className="size-1.5 rounded-full bg-current" />}</span>{label}</div>
}
