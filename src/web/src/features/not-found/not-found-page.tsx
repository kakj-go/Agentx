import { ArrowLeft, FileQuestion } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Link } from 'react-router-dom'

import { PageContainer } from '../../shared/components/page-container'
import { Button } from '../../shared/ui/button'

export function NotFoundPage() {
  const { t } = useTranslation()
  return (
    <PageContainer className="grid min-h-full place-items-center">
      <div className="text-center"><span className="mx-auto grid size-16 place-items-center rounded-2xl bg-primary/10 text-primary"><FileQuestion className="size-7" /></span><h1 className="mt-5 text-2xl font-bold">{t('navigation.notFound.title')}</h1><p className="mt-2 text-sm text-muted-foreground">{t('navigation.notFound.description')}</p><Button asChild className="mt-6"><Link to="/"><ArrowLeft className="size-4" />{t('navigation.notFound.back')}</Link></Button></div>
    </PageContainer>
  )
}
