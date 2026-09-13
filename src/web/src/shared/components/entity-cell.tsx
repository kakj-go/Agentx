import type { LucideIcon } from 'lucide-react'

export function EntityCell({ icon: Icon, name, detail }: { icon: LucideIcon; name: string; detail?: string }) {
  return (
    <div className="flex items-center gap-3">
      <span className="grid size-9 shrink-0 place-items-center rounded-lg bg-primary/10 text-primary"><Icon className="size-4" /></span>
      <span className="min-w-0"><strong className="block truncate text-xs text-foreground">{name}</strong>{detail && <span className="mt-1 block truncate text-[10px] text-muted-foreground">{detail}</span>}</span>
    </div>
  )
}
