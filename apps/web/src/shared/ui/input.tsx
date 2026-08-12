import { forwardRef, type InputHTMLAttributes } from 'react'

import { cn } from '../lib/cn'

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(function Input({ className, ...props }, ref) {
  return (
    <input
      className={cn('h-9 w-full rounded-lg border border-border bg-surface px-3 text-sm text-foreground outline-none transition-shadow placeholder:text-muted-foreground focus:border-primary/60 focus:ring-2 focus:ring-primary/15 disabled:cursor-not-allowed disabled:opacity-50', className)}
      ref={ref}
      {...props}
    />
  )
})
