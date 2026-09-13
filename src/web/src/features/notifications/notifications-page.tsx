import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Bell, CheckCheck } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router-dom'

import { apiRequest } from '../../shared/api/client'
import type { NotificationInbox } from '../../shared/api/types'
import { EmptyState } from '../../shared/components/empty-state'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { useLocaleFormat } from '../../shared/lib/locale-format'
import { Button } from '../../shared/ui/button'
import { Card } from '../../shared/ui/card'

export function NotificationsPage() {
  const { t } = useTranslation()
  const { formatDateTime } = useLocaleFormat()
  const navigate = useNavigate()
  const queryClient = useQueryClient()
  const inbox = useQuery({ queryKey: ['notifications'], queryFn: () => apiRequest<NotificationInbox>('/notifications'), refetchInterval: 30_000 })
  const read = useMutation({ mutationFn: (id?: string) => apiRequest(id ? `/notifications/${id}/read` : '/notifications/read-all', { method: 'POST' }), onSuccess: async () => queryClient.invalidateQueries({ queryKey: ['notifications'] }) })
  const open = async (id: string, path: string) => { await read.mutateAsync(id); navigate(path) }
  return <PageContainer><PageHeader action={inbox.data?.unreadCount ? <Button onClick={() => read.mutate(undefined)} variant="secondary"><CheckCheck className="size-4" />{t('notifications.readAll')}</Button> : undefined} description={t('notifications.notificationsDescription')} title={t('navigation.notifications')} />
    <Card className="mt-6 overflow-hidden">{inbox.data?.items.length ? <div className="divide-y divide-border">{inbox.data.items.map((item) => <button className="flex w-full items-start gap-4 px-5 py-4 text-left transition-colors hover:bg-muted/40" key={item.id} onClick={() => void open(item.id, item.targetPath)}><span className={`mt-1.5 size-2 shrink-0 rounded-full ${item.read ? 'bg-border' : item.tone === 'danger' ? 'bg-danger' : item.tone === 'warning' ? 'bg-warning' : item.tone === 'success' ? 'bg-success' : 'bg-primary'}`} /><span className="min-w-0 flex-1"><strong className="text-xs">{t(item.titleKey, item.arguments as Record<string, string>)}</strong><span className="mt-1 block text-[11px] leading-5 text-muted-foreground">{t(item.bodyKey, item.arguments as Record<string, string>)}</span></span><time className="text-[10px] text-muted-foreground">{formatDateTime(item.createdAt)}</time></button>)}</div> : <EmptyState icon={Bell} title={t('navigation.header.noNotifications')} description={t('notifications.noNotificationsDescription')} />}</Card>
  </PageContainer>
}
