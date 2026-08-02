import { useTranslation } from 'react-i18next'

import { Badge } from '../ui/badge'

export type StatusValue = 'active' | 'inactive' | 'published' | 'draft' | 'running' | 'success' | 'failed' | 'pending' | 'waiting' | 'completed' | 'synced' | 'syncing'

const tones = {
  active: 'success', published: 'success', success: 'success', completed: 'success', synced: 'success',
  running: 'primary', syncing: 'primary',
  pending: 'warning', waiting: 'warning', draft: 'neutral', inactive: 'neutral', failed: 'danger',
} as const

export function StatusBadge({ status }: { status: StatusValue }) {
  const { t } = useTranslation()
  return <Badge className="gap-1.5" tone={tones[status]}><span className="size-1.5 rounded-full bg-current" />{t(`common.${status}`)}</Badge>
}
