import { Bot, MessageSquarePlus, Send, UserRound } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { ComingSoonAction } from '../../shared/components/coming-soon-action'
import { PageContainer } from '../../shared/components/page-container'
import { PageHeader } from '../../shared/components/page-header'
import { Card } from '../../shared/ui/card'
import { Input } from '../../shared/ui/input'

export function PlaygroundPage() {
  const { t } = useTranslation()
  return (
    <PageContainer className="flex min-h-full flex-col">
      <PageHeader action={<ComingSoonAction variant="secondary"><MessageSquarePlus className="size-4" />{t('pages.playground.newSession')}</ComingSoonAction>} description={t('pages.playground.description')} title={t('pages.playground.title')} />
      <Card className="mt-6 grid min-h-[620px] flex-1 grid-cols-[280px_minmax(0,1fr)] overflow-hidden">
        <aside className="border-r border-border bg-muted/25">
          <div className="flex h-14 items-center border-b border-border px-4 text-sm font-semibold">{t('pages.playground.sessions')}</div>
          <div className="p-2">
            <button className="w-full rounded-lg bg-primary/10 p-3 text-left">
              <strong className="block text-xs text-primary">{t('mocks.app.customer')}</strong>
              <span className="mt-1 block truncate text-[10px] text-muted-foreground">Session · demo-0821</span>
            </button>
          </div>
        </aside>
        <section className="flex min-w-0 flex-col">
          <div className="flex h-14 items-center gap-3 border-b border-border px-5"><span className="grid size-8 place-items-center rounded-lg bg-primary/10 text-primary"><Bot className="size-4" /></span><div><strong className="block text-xs">{t('pages.playground.selectApp')}</strong><span className="text-[10px] text-success">API Preview</span></div></div>
          <div className="flex flex-1 flex-col items-center justify-center px-8 text-center">
            <span className="grid size-14 place-items-center rounded-2xl bg-muted text-muted-foreground"><UserRound className="size-6" /></span>
            <strong className="mt-4 text-sm">{t('pages.playground.apiPending')}</strong>
            <p className="mt-2 max-w-md text-xs leading-5 text-muted-foreground">{t('pages.playground.description')}</p>
          </div>
          <div className="border-t border-border p-4">
            <div className="flex gap-2"><Input disabled placeholder={t('pages.playground.composer')} /><ComingSoonAction aria-label={t('pages.playground.send')} size="icon"><Send className="size-4" /></ComingSoonAction></div>
          </div>
        </section>
      </Card>
    </PageContainer>
  )
}
