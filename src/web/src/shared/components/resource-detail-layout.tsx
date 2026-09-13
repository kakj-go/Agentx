import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { PageContainer } from './page-container'
import { PageHeader } from './page-header'
import { StatusBadge } from './status-badge'
import { localizedValue } from '../lib/localized-value'
import { Card } from '../ui/card'

type DetailItem = { label: string; value: ReactNode }

type Props = {
  actions?: ReactNode
  children?: ReactNode
  description: string
  details: DetailItem[]
  error?: unknown
  loading?: boolean
  name?: string
  status?: string
}

export function ResourceDetailLayout({ actions, children, description, details, error, loading, name, status }: Props) {
  const { t } = useTranslation()
  if (loading) return <PageContainer><p className="text-sm text-muted-foreground">{t('common.loading')}</p></PageContainer>
  if (error || !name) return <PageContainer><p className="text-sm text-danger">{error instanceof Error ? error.message : String(error ?? t('common.loadFailed'))}</p></PageContainer>
  return <PageContainer>
    <PageHeader action={actions} description={description} title={name} />
    <div className="mt-4"><StatusBadge label={localizedValue(t, 'common', status)} status={status === 'active' ? 'active' : status === 'draft' ? 'draft' : 'inactive'} /></div>
    <div className="mt-6 space-y-5">
      <Card className="p-5">
        <dl className="grid grid-cols-2 gap-x-6 gap-y-5">
          {details.map((item) => <div key={item.label}><dt className="text-[11px] text-muted-foreground">{item.label}</dt><dd className="mt-1 break-all text-xs font-medium">{item.value || '—'}</dd></div>)}
        </dl>
      </Card>
      {children}
    </div>
  </PageContainer>
}
