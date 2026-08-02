import { Bot } from 'lucide-react'
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'

import { Card } from '../../shared/ui/card'

export function AuthShell({ eyebrow, title, description, children }: { eyebrow: string; title: string; description: string; children: ReactNode }) {
  const { t } = useTranslation()
  return <main className="grid min-h-screen min-w-[960px] grid-cols-[minmax(360px,0.9fr)_minmax(520px,1.1fr)] bg-background">
    <section className="flex flex-col justify-between bg-primary p-12 text-primary-foreground">
      <div className="flex items-center gap-3"><span className="grid size-10 place-items-center rounded-xl bg-primary-foreground/15"><Bot className="size-5" /></span><strong className="text-lg">Agentx</strong></div>
      <div className="max-w-md"><p className="text-xs font-semibold uppercase tracking-[.2em] text-primary-foreground/65">{eyebrow}</p><h1 className="mt-4 text-4xl font-semibold leading-tight">{t('auth.brand')}</h1><p className="mt-5 text-sm leading-7 text-primary-foreground/70">{description}</p></div>
      <p className="text-xs text-primary-foreground/55">Agentx · Workflow Cloud</p>
    </section>
    <section className="grid place-items-center p-12"><Card className="w-full max-w-lg p-8"><h2 className="text-2xl font-semibold tracking-tight">{title}</h2><p className="mt-2 text-sm leading-6 text-muted-foreground">{description}</p><div className="mt-7">{children}</div></Card></section>
  </main>
}
