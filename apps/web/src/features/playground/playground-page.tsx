import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Bot, ExternalLink, MessageSquarePlus, Send, Square, UserRound } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { apiRequest, gatewayRequest, jsonBody } from '../../shared/api/client'
import type { Application, GatewayInvocation, GatewayMessage, GatewaySession, PageResponse } from '../../shared/api/types'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { useLocaleFormat } from '../../shared/lib/locale-format'
import { localizedValue } from '../../shared/lib/localized-value'
import { Badge } from '../../shared/ui/badge'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { Input } from '../../shared/ui/input'
import { Select } from '../../shared/ui/select'
import { useToast } from '../../shared/ui/toast'
import { useInvocationEvents } from './use-invocation-events'

const terminalStatuses = new Set(['completed', 'failed', 'cancelled'])

export function PlaygroundPage() {
  const { t } = useTranslation()
  const { formatTime } = useLocaleFormat()
  const { showToast } = useToast()
  const queryClient = useQueryClient()
  const [applicationId, setApplicationId] = useState('')
  const [session, setSession] = useState<GatewaySession | null>(null)
  const [message, setMessage] = useState('')
  const [invocationId, setInvocationId] = useState<string>()
  const applications = useQuery({ queryKey: ['applications', 'playground'], queryFn: () => apiRequest<PageResponse<Application>>('/applications?pageSize=100&status=active') })
  const selected = applications.data?.items.find((item) => item.id === applicationId)
  const invocation = useQuery({
    queryKey: ['gateway-invocation', invocationId],
    queryFn: () => gatewayRequest<GatewayInvocation>(`/invocations/${invocationId}`),
    enabled: Boolean(invocationId),
    refetchInterval: (query) => query.state.data && terminalStatuses.has(query.state.data.status) ? false : 2_000,
  })
  const messages = useQuery({
    queryKey: ['gateway-messages', session?.id],
    queryFn: () => gatewayRequest<GatewayMessage[]>(`/sessions/${session?.id}/messages`),
    enabled: Boolean(session),
  })
  const running = Boolean(invocation.data && !terminalStatuses.has(invocation.data.status))
  useInvocationEvents(invocationId, running, () => {
    void queryClient.invalidateQueries({ queryKey: ['gateway-invocation', invocationId] })
    void queryClient.invalidateQueries({ queryKey: ['gateway-messages', session?.id] })
  })
  const createSession = useMutation({
    mutationFn: () => gatewayRequest<GatewaySession>(`/applications/${selected?.slug}/sessions`, { method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() }, body: jsonBody({ title: t('applications.playgroundSession'), externalUserId: null }) }),
    onSuccess: (value) => { setSession(value); setInvocationId(undefined) },
    onError: (error: Error) => showToast(error.message),
  })
  const send = useMutation({
    mutationFn: () => gatewayRequest<GatewayInvocation>(`/sessions/${session?.id}/messages`, { method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() }, body: jsonBody({ parts: [{ partType: 'text', content: message }] }) }),
    onSuccess: async (value) => {
      setInvocationId(value.id)
      setMessage('')
      await queryClient.invalidateQueries({ queryKey: ['gateway-messages', session?.id] })
    },
    onError: (error: Error) => showToast(error.message),
  })
  const cancel = useMutation({
    mutationFn: () => gatewayRequest(`/invocations/${invocationId}/cancel`, { method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() } }),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['gateway-invocation', invocationId] }),
    onError: (error: Error) => showToast(error.message),
  })

  return <PageContainer className="flex min-h-full flex-col">
    <PageHeader action={<div className="flex items-center gap-2"><Select className="w-64" onValueChange={(value) => { setApplicationId(value); setSession(null); setInvocationId(undefined) }} options={(applications.data?.items ?? []).map((item) => ({ value: item.id, label: item.name }))} placeholder={t('applications.selectApplication')} value={applicationId} /><Button disabled={!selected || createSession.isPending} onClick={() => createSession.mutate()} variant="secondary"><MessageSquarePlus className="size-4" />{t('applications.playground.newSession')}</Button></div>} description={t('applications.playground.description')} title={t('applications.playground.title')} />
    <Card className="mt-6 grid min-h-[620px] flex-1 grid-cols-[280px_minmax(0,1fr)] overflow-hidden">
      <aside className="border-r border-border bg-muted/25"><div className="flex h-14 items-center border-b border-border px-4 text-sm font-semibold">{t('applications.playground.sessions')}</div><div className="p-2">{session ? <div className="rounded-md bg-primary/10 p-3"><strong className="block text-xs text-primary">{session.title ?? selected?.name}</strong><span className="mt-1 block truncate text-[10px] text-muted-foreground">{session.id}</span></div> : <p className="p-4 text-xs text-muted-foreground">{t('applications.noSession')}</p>}</div></aside>
      <section className="flex min-w-0 flex-col">
        <div className="flex h-14 items-center gap-3 border-b border-border px-5"><span className="grid size-8 place-items-center rounded-md bg-primary/10 text-primary"><Bot className="size-4" /></span><div><strong className="block text-xs">{selected?.name ?? t('applications.selectApplication')}</strong><span className="text-[10px] text-muted-foreground">{session ? localizedValue(t, 'applications', session.versionPolicy) : '/gateway/v1'}</span></div>{invocation.data && <><Badge className="ml-auto" tone={invocation.data.status === 'failed' ? 'danger' : running ? 'warning' : 'success'}>{localizedValue(t, 'common', invocation.data.status)}</Badge>{invocation.data.executionId && <Button asChild size="icon" variant="ghost"><Link aria-label={t('applications.trace')} to={`/executions/${invocation.data.executionId}`}><ExternalLink className="size-4" /></Link></Button>}{running && <Button aria-label={t('studio.stop')} disabled={cancel.isPending} onClick={() => cancel.mutate()} size="icon" variant="ghost"><Square className="size-4" /></Button>}</>}</div>
        {primaryInvocationError(invocation.data?.error) && <div className="border-b border-danger/25 bg-danger/10 px-5 py-2 text-xs text-danger"><strong>{primaryInvocationError(invocation.data?.error)?.code ?? 'APPLICATION_INVOCATION_FAILED'}</strong><span className="ml-2">{primaryInvocationError(invocation.data?.error)?.message}</span></div>}
        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">{!session ? <EmptyConversation icon={<UserRound className="size-6" />} title={t('applications.selectAndCreate')} /> : messages.isLoading ? <p className="text-xs text-muted-foreground">{t('common.loading')}</p> : (messages.data?.length ?? 0) === 0 ? <EmptyConversation icon={<Bot className="size-6" />} title={t('runtime.noMessages')} /> : <div className="space-y-4">{messages.data?.map((item) => <article className={`flex gap-3 ${item.role === 'user' ? 'justify-end' : 'justify-start'}`} key={item.id}><div className={`max-w-[75%] rounded-md px-4 py-3 text-xs leading-5 ${item.role === 'user' ? 'bg-primary text-primary-foreground' : 'border border-border bg-muted/40 text-foreground'}`}>{item.parts.map((part, index) => <div className="whitespace-pre-wrap break-words" key={`${part.partType}-${index}`}>{formatPart(part.content)}</div>)}<span className={`mt-2 block text-[10px] ${item.role === 'user' ? 'text-primary-foreground/70' : 'text-muted-foreground'}`}>{formatTime(item.createdAt)}</span></div></article>)}</div>}</div>
        <div className="border-t border-border p-4"><div className="flex gap-2"><Input disabled={!session || running} onChange={(event) => setMessage(event.target.value)} onKeyDown={(event) => { if (event.key === 'Enter' && message.trim() && !send.isPending && !running) send.mutate() }} placeholder={t('applications.playground.composer')} value={message} /><Button aria-label={t('applications.playground.send')} disabled={!session || !message.trim() || send.isPending || running} onClick={() => send.mutate()} size="icon"><Send className="size-4" /></Button></div></div>
      </section>
    </Card>
  </PageContainer>
}

function EmptyConversation({ icon, title }: { icon: React.ReactNode; title: string }) {
  return <div className="flex h-full flex-col items-center justify-center text-center text-muted-foreground"><span className="grid size-12 place-items-center rounded-md bg-muted">{icon}</span><strong className="mt-3 text-xs font-medium text-foreground">{title}</strong></div>
}

function formatPart(content: unknown) {
  return typeof content === 'string' ? content : JSON.stringify(content, null, 2)
}

function primaryInvocationError(value: unknown): { code?: string; message?: string } | undefined {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined
  const primary = (value as { primaryError?: unknown }).primaryError
  if (!primary || typeof primary !== 'object' || Array.isArray(primary)) return undefined
  return primary as { code?: string; message?: string }
}
