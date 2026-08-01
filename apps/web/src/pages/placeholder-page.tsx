import { Construction } from 'lucide-react'

type PlaceholderPageProps = {
  title: string
  description: string
}

export function PlaceholderPage({ title, description }: PlaceholderPageProps) {
  return (
    <div className="mx-auto w-full max-w-[1480px] p-6 lg:p-8">
      <h1 className="text-2xl font-bold tracking-tight">{title}</h1>
      <p className="mt-2 text-sm text-muted-foreground">{description}</p>
      <div className="mt-7 flex min-h-72 flex-col items-center justify-center rounded-xl border border-dashed border-border bg-surface">
        <span className="grid size-12 place-items-center rounded-xl bg-primary/10 text-primary"><Construction className="size-5" /></span>
        <strong className="mt-4 text-sm">基础路由已经就绪</strong>
        <span className="mt-1 text-xs text-muted-foreground">业务能力将在对应开发阶段接入。</span>
      </div>
    </div>
  )
}

