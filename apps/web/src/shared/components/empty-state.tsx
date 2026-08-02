import { SearchX } from 'lucide-react'
import { useTranslation } from 'react-i18next'

export function EmptyState() {
  const { t } = useTranslation()
  return (
    <div className="flex min-h-56 flex-col items-center justify-center px-6 text-center">
      <span className="grid size-11 place-items-center rounded-xl bg-muted text-muted-foreground"><SearchX className="size-5" /></span>
      <strong className="mt-4 text-sm">{t('common.noResults')}</strong>
      <span className="mt-1 text-xs text-muted-foreground">{t('common.noResultsHint')}</span>
    </div>
  )
}
