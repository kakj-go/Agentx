import type { ReactNode } from 'react'

type PageHeaderProps = { title: string; description: string; eyebrow?: string; action?: ReactNode }

export function PageHeader({ title, description, eyebrow, action }: PageHeaderProps) {
  return (
    <header className="flex items-end justify-between gap-6">
      <div>
        {eyebrow && <p className="mb-2 text-[10px] font-semibold uppercase tracking-[0.14em] text-muted-foreground">{eyebrow}</p>}
        <h1 className="text-2xl font-bold tracking-tight">{title}</h1>
        <p className="mt-2 text-sm text-muted-foreground">{description}</p>
      </div>
      {action && <div className="shrink-0">{action}</div>}
    </header>
  )
}
