import { useMutation, useQuery } from '@tanstack/react-query'
import { Bot, MessageSquarePlus, Send, UserRound } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { ApiClientError, apiRequest, gatewayRequest, jsonBody } from '../../shared/api/client'
import type { Application, GatewayInvocation, GatewaySession, PageResponse } from '../../shared/api/types'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'
import { Input } from '../../shared/ui/input'
import { Select } from '../../shared/ui/select'
import { useToast } from '../../shared/ui/toast'

export function PlaygroundPage() {
  const { t } = useTranslation()
  const { showToast } = useToast()
  const [applicationId, setApplicationId] = useState('')
  const [session, setSession] = useState<GatewaySession | null>(null)
  const [message, setMessage] = useState('')
  const applications = useQuery({ queryKey: ['applications', 'playground'], queryFn: () => apiRequest<PageResponse<Application>>('/applications?pageSize=100&status=active') })
  const selected = applications.data?.items.find((item) => item.id === applicationId)
  const createSession = useMutation({ mutationFn: () => gatewayRequest<GatewaySession>(`/applications/${selected?.slug}/sessions`, { method: 'POST', body: jsonBody({ title: t('m3.playgroundSession'), externalUserId: null }) }), onSuccess: setSession, onError: (error: Error) => showToast(error.message) })
  const send = useMutation({ mutationFn: () => gatewayRequest<GatewayInvocation>(`/sessions/${session?.id}/messages`, { method: 'POST', headers: { 'Idempotency-Key': crypto.randomUUID() }, body: jsonBody({ parts: [{ partType: 'text', content: message }] }) }), onError: (error: Error) => showToast(error instanceof ApiClientError && error.detail.code === 'RUNTIME_UNAVAILABLE' ? t('m3.runtimeUnavailable') : error.message) })
  return <PageContainer className="flex min-h-full flex-col"><PageHeader action={<div className="flex items-center gap-2"><Select className="w-64" onValueChange={(value) => { setApplicationId(value); setSession(null) }} options={(applications.data?.items ?? []).map((item) => ({ value: item.id, label: item.name }))} placeholder={t('m3.selectApplication')} value={applicationId} /><Button disabled={!selected} onClick={() => createSession.mutate()} variant="secondary"><MessageSquarePlus className="size-4" />{t('pages.playground.newSession')}</Button></div>} description={t('pages.playground.description')} title={t('pages.playground.title')} /><Card className="mt-6 grid min-h-[620px] flex-1 grid-cols-[280px_minmax(0,1fr)] overflow-hidden"><aside className="border-r border-border bg-muted/25"><div className="flex h-14 items-center border-b border-border px-4 text-sm font-semibold">{t('pages.playground.sessions')}</div><div className="p-2">{session ? <div className="rounded-lg bg-primary/10 p-3"><strong className="block text-xs text-primary">{session.title ?? selected?.name}</strong><span className="mt-1 block truncate text-[10px] text-muted-foreground">{session.id}</span></div> : <p className="p-4 text-xs text-muted-foreground">{t('m3.noSession')}</p>}</div></aside><section className="flex min-w-0 flex-col"><div className="flex h-14 items-center gap-3 border-b border-border px-5"><span className="grid size-8 place-items-center rounded-lg bg-primary/10 text-primary"><Bot className="size-4" /></span><div><strong className="block text-xs">{selected?.name ?? t('m3.selectApplication')}</strong><span className="text-[10px] text-success">/gateway/v1</span></div></div><div className="flex flex-1 flex-col items-center justify-center px-8 text-center"><span className="grid size-14 place-items-center rounded-2xl bg-muted text-muted-foreground"><UserRound className="size-6" /></span><strong className="mt-4 text-sm">{session ? t('m3.sessionReady') : t('m3.selectAndCreate')}</strong><p className="mt-2 max-w-md text-xs leading-5 text-muted-foreground">{t('m3.runtimeUnavailableDescription')}</p></div><div className="border-t border-border p-4"><div className="flex gap-2"><Input disabled={!session} onChange={(event) => setMessage(event.target.value)} placeholder={t('pages.playground.composer')} value={message} /><Button aria-label={t('pages.playground.send')} disabled={!session || !message.trim() || send.isPending} onClick={() => send.mutate()} size="icon"><Send className="size-4" /></Button></div></div></section></Card></PageContainer>
}
