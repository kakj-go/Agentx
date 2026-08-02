import { ChevronDown } from 'lucide-react'
import type { SelectHTMLAttributes } from 'react'

import { cn } from '../lib/cn'

export function Select({ className, children, ...props }: SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <span className="relative inline-flex">
      <select className={cn('h-9 appearance-none rounded-lg border border-border bg-surface py-0 pl-3 pr-9 text-sm text-foreground outline-none focus:border-primary/60 focus:ring-2 focus:ring-primary/15', className)} {...props}>
        {children}
      </select>
      <ChevronDown className="pointer-events-none absolute right-3 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
    </span>
  )
}
