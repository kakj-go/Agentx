import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ArrowLeft, Ban, GitFork, Link2, RefreshCw } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router-dom'

import { useAuth } from '../../app/providers/auth-provider'
import { apiRequest, apiRequestBlob, jsonBody } from '../../shared/api/client'
import type { Approval, Checkpoint, Execution, ExecutionWait, ForkExecutionRequest, NodeExecution, PageResponse, RuntimeDetails, SideEffectConfirmationRequest, Trace } from '../../shared/api/types'
import { ConfirmDialog } from '../../shared/components/confirm-dialog'
import { EmptyState } from '../../shared/components/empty-state'
import { StatusBadge } from '../../shared/components/status-badge'
import { Button } from '../../shared/ui/button'
import { useToast } from '../../shared/ui/toast'
import { executionStatus } from './execution-format'
import { ExecutionForkDialog } from './execution-fork-dialog'
import { ExecutionNodePanel } from './execution-node-panel'
import { ExecutionOutline } from './execution-outline'
import { ExecutionRecoveryRail } from './execution-recovery-rail'
import { ExecutionRuntimePanel } from './execution-runtime-panel'
import { SideEffectDialog } from './side-effect-dialog'

type ItemResponse<T> = { items: T[] }
type CommandResponse = { executionId: string; status: string; replayed: boolean }

const terminalStatuses = new Set(['succeeded', 'failed', 'cancelled', 'timed_out'])

export function ExecutionDetailPage() {
  const { id = '' } = useParams()
  const auth = useAuth()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const { showToast } = useToast()
  const [selectedId, setSelectedId] = useState<string>()
  const [forkOpen, setForkOpen] = useState(false)
  const [cancelOpen, setCancelOpen] = useState(false)
  const [confirmationNode, setConfirmationNode] = useState<NodeExecution>()
  const finalizedExecution = useRef<string>()
  const forkTrigger = useRef<HTMLButtonElement>(null)

  const execution = useQuery({
    queryKey: ['execution', id],
    queryFn: () => apiRequest<Execution>(`/executions/${id}`),
    refetchInterval: (query) => terminalStatuses.has(query.state.data?.status ?? '') ? false : 2_000,
  })
  const nodes = useQuery({ queryKey: ['execution-nodes', id], queryFn: () => apiRequest<ItemResponse<NodeExecution>>(`/executions/${id}/nodes`), refetchInterval: execution.data && !terminalStatuses.has(execution.data.status) ? 2_000 : false })
  const nodeDetail = useQuery({
    enabled: Boolean(selectedId),
    queryKey: ['execution-node', id, selectedId],
    queryFn: () => apiRequest<NodeExecution>(`/executions/${id}/nodes/${selectedId}`),
    refetchInterval: execution.data && !terminalStatuses.has(execution.data.status) ? 2_000 : false,
  })
  const checkpoints = useQuery({ queryKey: ['execution-checkpoints', id], queryFn: () => apiRequest<ItemResponse<Checkpoint>>(`/executions/${id}/checkpoints`), refetchInterval: execution.data && !terminalStatuses.has(execution.data.status) ? 2_000 : false })
  const waits = useQuery({ queryKey: ['execution-waits', id], queryFn: () => apiRequest<ItemResponse<ExecutionWait>>(`/executions/${id}/waits`), refetchInterval: execution.data && !terminalStatuses.has(execution.data.status) ? 2_000 : false })
  const trace = useQuery({ queryKey: ['execution-trace', id], queryFn: () => apiRequest<Trace>(`/executions/${id}/trace?limit=200`), retry: false, refetchInterval: execution.data && !terminalStatuses.has(execution.data.status) ? 3_000 : false })
  const runtimeDetails = useQuery({ queryKey: ['execution-runtime-details', id], queryFn: () => apiRequest<RuntimeDetails>(`/executions/${id}/runtime-details`), refetchInterval: execution.data && !terminalStatuses.has(execution.data.status) ? 2_000 : false })
  const approvals = useQuery({ enabled: auth.hasPermission('approval:view'), queryKey: ['approvals', 'execution', id], queryFn: () => apiRequest<PageResponse<Approval>>('/approvals?pageSize=100'), refetchInterval: execution.data && !terminalStatuses.has(execution.data.status) ? 2_000 : false })

  useEffect(() => {
    const status = execution.data?.status
    if (!status || !terminalStatuses.has(status)) return
    const terminalKey = `${id}:${status}`
    if (finalizedExecution.current === terminalKey) return
    finalizedExecution.current = terminalKey
    void Promise.all([
      queryClient.invalidateQueries({ queryKey: ['execution-nodes', id] }),
      queryClient.invalidateQueries({ queryKey: ['execution-node', id] }),
      queryClient.invalidateQueries({ queryKey: ['execution-checkpoints', id] }),
      queryClient.invalidateQueries({ queryKey: ['execution-waits', id] }),
      queryClient.invalidateQueries({ queryKey: ['execution-trace', id] }),
      queryClient.invalidateQueries({ queryKey: ['execution-runtime-details', id] }),
      queryClient.invalidateQueries({ queryKey: ['approvals', 'execution', id] }),
    ])
  }, [execution.data?.status, id, queryClient])

  const nodeItems = useMemo(() => nodes.data?.items ?? [], [nodes.data?.items])
  useEffect(() => {
    if (!selectedId && nodeItems.length) setSelectedId(nodeItems.at(-1)?.id)
    if (selectedId && nodeItems.length && !nodeItems.some((node) => node.id === selectedId)) setSelectedId(nodeItems.at(-1)?.id)
  }, [nodeItems, selectedId])

  const refresh = async () => Promise.all([
    queryClient.invalidateQueries({ queryKey: ['execution', id] }),
    queryClient.invalidateQueries({ queryKey: ['execution-nodes', id] }),
    queryClient.invalidateQueries({ queryKey: ['execution-checkpoints', id] }),
    queryClient.invalidateQueries({ queryKey: ['execution-waits', id] }),
    queryClient.invalidateQueries({ queryKey: ['execution-trace', id] }),
    queryClient.invalidateQueries({ queryKey: ['execution-runtime-details', id] }),
    queryClient.invalidateQueries({ queryKey: ['executions'] }),
  ])
  const closeFork = () => {
    setForkOpen(false)
    requestAnimationFrame(() => forkTrigger.current?.focus())
  }
  const cancel = useMutation({
    mutationFn: () => apiRequest(`/executions/${id}/cancel`, { method: 'POST' }),
    onSuccess: async () => { setCancelOpen(false); await refresh(); showToast('Execution 已取消') },
    onError: (error: Error) => showToast(error.message),
  })
  const fork = useMutation({
    mutationFn: (request: ForkExecutionRequest) => apiRequest<CommandResponse>(`/executions/${id}/fork`, { method: 'POST', body: jsonBody(request) }),
    onSuccess: async (result) => { setForkOpen(false); await queryClient.invalidateQueries({ queryKey: ['executions'] }); navigate(`/executions/${result.executionId}`) },
  })
  const confirmation = useMutation({
    mutationFn: (request: SideEffectConfirmationRequest) => apiRequest(`/executions/${id}/side-effect-confirmations`, { method: 'POST', body: jsonBody(request) }),
    onSuccess: async () => { setConfirmationNode(undefined); await refresh(); showToast('副作用决策已提交') },
    onError: (error: Error) => showToast(error.message),
  })

  if (!execution.data) return <div className="p-6"><EmptyState title={execution.isLoading ? '正在加载 Execution' : execution.error instanceof Error && execution.error.message.includes('403') ? '没有查看权限' : 'Execution 加载失败'} description={execution.error instanceof Error ? execution.error.message : '正在读取运行快照、节点和恢复状态。'} /></div>

  const value = execution.data
  const active = !terminalStatuses.has(value.status)
  const selectedNode = nodeDetail.data ?? nodeItems.find((node) => node.id === selectedId)
  const executionApprovals = approvals.data?.items.filter((approval) => approval.executionId === id) ?? []
  const downloadArtifact = async (artifactId: string) => {
    try {
      const blob = await apiRequestBlob(`/executions/${id}/artifacts/${artifactId}`)
      const url = URL.createObjectURL(blob)
      const anchor = document.createElement('a')
      anchor.href = url
      anchor.download = artifactId
      anchor.click()
      URL.revokeObjectURL(url)
    } catch (error) { showToast(error instanceof Error ? error.message : String(error)) }
  }

  return <div className="min-h-full bg-background">
    <header className="border-b border-border bg-surface px-4 py-3 lg:px-6">
      <div className="grid grid-cols-[2.25rem_minmax(0,1fr)] items-center gap-x-3 gap-y-2 md:flex md:flex-wrap md:gap-3">
        <Button asChild aria-label="返回执行列表" size="icon" variant="ghost"><Link to="/executions"><ArrowLeft className="size-4" /></Link></Button>
        <div className="min-w-0 md:flex-1"><div className="flex min-w-0 items-center gap-2"><h1 className="min-w-0 flex-1 truncate text-base font-semibold">{value.workflowName}</h1><StatusBadge label={value.status} status={executionStatus(value.status)} /></div><p className="mt-1 truncate text-[10px] text-muted-foreground">Execution {value.id} · Version {value.workflowVersionNumber} · {value.executionType}</p></div>
        <div className="col-start-2 flex flex-wrap items-center gap-x-5 gap-y-1 text-[10px] text-muted-foreground md:col-auto"><span><strong className="block text-xs text-foreground">{duration(value)}</strong>Duration</span><span><strong className="block text-xs text-foreground">{value.triggerType}</strong>Trigger</span><span><strong className="block max-w-36 truncate text-xs text-foreground">{value.traceId}</strong>Trace</span></div>
        <div className="col-start-2 flex items-center justify-self-end gap-2 md:col-auto md:ml-auto">
          <Button aria-label="刷新 Execution" onClick={() => void refresh()} size="icon" variant="ghost"><RefreshCw className="size-4" /></Button>
          {auth.hasPermission('execution:cancel') && active && <Button onClick={() => setCancelOpen(true)} variant="secondary"><Ban className="size-4" />Cancel</Button>}
          {auth.hasPermission('execution:fork') && <Button disabled={!checkpoints.data?.items.length} onClick={() => setForkOpen(true)} ref={forkTrigger}><GitFork className="size-4" />Fork</Button>}
        </div>
      </div>
      {(value.parentExecutionId || value.callerExecutionId) && <div className="mt-2 flex flex-wrap gap-3 border-t border-border pt-2 text-[10px] text-muted-foreground">{value.parentExecutionId && <Link className="inline-flex items-center gap-1 text-primary hover:underline" to={`/executions/${value.parentExecutionId}`}><Link2 className="size-3" />Parent {value.parentExecutionId}</Link>}{value.callerExecutionId && <Link className="inline-flex items-center gap-1 text-primary hover:underline" to={`/executions/${value.callerExecutionId}`}><Link2 className="size-3" />Caller {value.callerExecutionId}</Link>}</div>}
      {value.errorMessage && <p className="mt-3 border-l-2 border-danger bg-danger/10 px-3 py-2 text-xs text-danger"><strong>{value.errorCode}</strong> {value.errorMessage}</p>}
    </header>

    <div className="grid min-h-[620px] grid-cols-[240px_minmax(420px,1fr)_320px] overflow-hidden max-xl:grid-cols-[230px_minmax(0,1fr)] max-lg:grid-cols-1">
      <ExecutionOutline nodes={nodeItems} onSelect={setSelectedId} selectedId={selectedId} />
      <ExecutionNodePanel node={selectedNode} onDownloadArtifact={(artifactId) => void downloadArtifact(artifactId)} trace={trace.data} />
      <div className="max-xl:col-span-2 max-lg:col-span-1"><ExecutionRecoveryRail approvals={executionApprovals} canConfirm={auth.hasPermission('execution:fork')} checkpoints={checkpoints.data?.items ?? []} nodes={nodeItems} onConfirm={setConfirmationNode} onDownloadArtifact={(artifactId) => void downloadArtifact(artifactId)} trace={trace.data} waits={waits.data?.items ?? []} /></div>
    </div>

    <ExecutionRuntimePanel details={runtimeDetails.data} error={runtimeDetails.error} loading={runtimeDetails.isLoading} onDownloadArtifact={(artifactId) => void downloadArtifact(artifactId)} />

    <ExecutionForkDialog checkpoints={checkpoints.data?.items ?? []} initialNodeId={selectedNode?.nodeId} nodes={nodeItems} onClose={closeFork} onSubmit={(request) => fork.mutateAsync(request).then(() => undefined)} open={forkOpen} pending={fork.isPending} />
    <SideEffectDialog checkpointId={value.forkCheckpointId ?? undefined} node={confirmationNode} onClose={() => setConfirmationNode(undefined)} onSubmit={(request) => confirmation.mutateAsync(request).then(() => undefined)} pending={confirmation.isPending} />
    <ConfirmDialog cancelLabel="返回" confirmLabel="Cancel execution" description="取消会持久化终止状态、释放 Lease，并使后续 Resume 失效。此操作不会删除运行历史。" onClose={() => setCancelOpen(false)} onConfirm={() => cancel.mutateAsync().then(() => undefined)} open={cancelOpen} pending={cancel.isPending} title="Cancel execution?" />
  </div>
}

function duration(execution: Execution) {
  const milliseconds = execution.durationMs ?? (Date.now() - new Date(execution.startedAt).getTime())
  if (milliseconds < 1_000) return `${milliseconds} ms`
  if (milliseconds < 60_000) return `${(milliseconds / 1_000).toFixed(1)} s`
  return `${Math.floor(milliseconds / 60_000)}m ${Math.floor((milliseconds % 60_000) / 1_000)}s`
}
