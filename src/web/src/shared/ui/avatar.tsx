import type { HTMLAttributes } from 'react'

import { cn } from '../lib/cn'

type AvatarProps = HTMLAttributes<HTMLSpanElement> & { initials: string; tone?: 'primary' | 'dark' }

export function Avatar({ className, initials, tone = 'primary', ...props }: AvatarProps) {
  return (
    <span className={cn('inline-grid size-8 shrink-0 place-items-center rounded-full text-[10px] font-bold', tone === 'primary' ? 'bg-primary/12 text-primary' : 'bg-foreground text-background', className)} {...props}>
      {initials}
    </span>
  )
}
