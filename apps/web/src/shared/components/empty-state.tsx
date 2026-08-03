import { SearchX, type LucideIcon } from 'lucide-react'
import { useTranslation } from 'react-i18next'

type EmptyStateProps = { title?: string; description?: string; icon?: LucideIcon }

export function EmptyState({ title, description, icon: Icon = SearchX }: EmptyStateProps = {}) {
  const { t } = useTranslation()
  return (
    <div className="flex min-h-56 flex-col items-center justify-center px-6 text-center">
      <span className="grid size-11 place-items-center rounded-xl bg-muted text-muted-foreground"><Icon className="size-5" /></span>
      <strong className="mt-4 text-sm">{title ?? t('common.noResults')}</strong>
      <span className="mt-1 text-xs text-muted-foreground">{description ?? t('common.noResultsHint')}</span>
    </div>
  )
}
