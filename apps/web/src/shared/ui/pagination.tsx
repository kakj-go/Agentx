import { useTranslation } from 'react-i18next'

import { Button } from './button'

type PaginationProps = { page: number; pageCount: number; onPageChange: (page: number) => void }

export function Pagination({ page, pageCount, onPageChange }: PaginationProps) {
  const { t } = useTranslation()
  return (
    <div className="flex items-center gap-3">
      <span className="text-[10px] text-muted-foreground">{t('common.page', { current: page + 1, total: pageCount })}</span>
      <Button disabled={page === 0} onClick={() => onPageChange(page - 1)} size="sm" variant="secondary">{t('common.previous')}</Button>
      <Button disabled={page + 1 >= pageCount} onClick={() => onPageChange(page + 1)} size="sm" variant="secondary">{t('common.next')}</Button>
    </div>
  )
}
