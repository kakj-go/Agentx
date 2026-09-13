import { forwardRef, type InputHTMLAttributes } from 'react'

import { cn } from '../lib/cn'

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(function Input({ className, ...props }, ref) {
  return (
    <input
      className={cn('h-9 w-full rounded-lg border border-border bg-surface px-3 text-sm text-foreground outline-none transition-[border-color,box-shadow] placeholder:text-muted-foreground focus:border-primary focus:ring-2 focus:ring-primary/20 aria-invalid:border-danger aria-invalid:ring-2 aria-invalid:ring-danger/15 aria-invalid:focus:border-danger aria-invalid:focus:ring-danger/20 disabled:cursor-not-allowed disabled:opacity-50', className)}
      ref={ref}
      {...props}
    />
  )
})
