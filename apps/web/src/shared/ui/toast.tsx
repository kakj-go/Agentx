/* oxlint-disable react/only-export-components */
import * as ToastPrimitive from '@radix-ui/react-toast'
import { X } from 'lucide-react'
import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react'

type ToastContextValue = { showToast: (message: string) => void }

const ToastContext = createContext<ToastContextValue | null>(null)

export function ToastProvider({ children }: { children: ReactNode }) {
  const [message, setMessage] = useState('')
  const [open, setOpen] = useState(false)
  const showToast = useCallback((next: string) => {
    setMessage(next)
    setOpen(false)
    window.setTimeout(() => setOpen(true), 0)
  }, [])
  const value = useMemo(() => ({ showToast }), [showToast])

  return (
    <ToastContext.Provider value={value}>
      <ToastPrimitive.Provider duration={3200} swipeDirection="right">
        {children}
        <ToastPrimitive.Root className="grid grid-cols-[1fr_auto] items-center gap-4 rounded-xl border border-border bg-surface px-4 py-3 text-foreground shadow-xl" onOpenChange={setOpen} open={open}>
          <ToastPrimitive.Description className="text-xs">{message}</ToastPrimitive.Description>
          <ToastPrimitive.Close className="rounded-md p-1 text-muted-foreground hover:bg-muted hover:text-foreground"><X className="size-3.5" /></ToastPrimitive.Close>
        </ToastPrimitive.Root>
        <ToastPrimitive.Viewport className="fixed bottom-5 right-5 z-[100] flex w-96 max-w-[calc(100vw-40px)] flex-col gap-2 outline-none" />
      </ToastPrimitive.Provider>
    </ToastContext.Provider>
  )
}

export function useToast() {
  const context = useContext(ToastContext)
  if (!context) throw new Error('useToast must be used inside ToastProvider')
  return context
}
