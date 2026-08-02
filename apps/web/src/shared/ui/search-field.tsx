import { Search } from 'lucide-react'
import type { InputHTMLAttributes } from 'react'

import { cn } from '../lib/cn'
import { Input } from './input'

export function SearchField({ className, ...props }: InputHTMLAttributes<HTMLInputElement>) {
  return <span className={cn('relative block w-72', className)}><Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" /><Input className="pl-9" type="search" {...props} /></span>
}
