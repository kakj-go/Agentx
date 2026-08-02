import { useState, type FormEvent, type ReactNode } from 'react'

import { ApiClientError } from '../../shared/api/client'
import { Button } from '../../shared/ui/button'
import { Input } from '../../shared/ui/input'

export function AuthForm({ children, submitLabel, onSubmit }: { children: ReactNode; submitLabel: string; onSubmit: () => Promise<void> }) {
  const [pending, setPending] = useState(false); const [error, setError] = useState('')
  const submit = async (event: FormEvent) => { event.preventDefault(); setPending(true); setError(''); try { await onSubmit() } catch (value) { setError(value instanceof ApiClientError ? value.message : String(value)) } finally { setPending(false) } }
  return <form className="space-y-5" onSubmit={(event) => void submit(event)}>{children}{error && <p className="rounded-lg border border-danger/25 bg-danger/10 px-3 py-2 text-xs text-danger">{error}</p>}<Button className="w-full" disabled={pending} type="submit">{pending ? '…' : submitLabel}</Button></form>
}
export function Field({ label, ...props }: { label: string } & React.ComponentProps<typeof Input>) { return <label className="block text-xs font-medium"><span className="mb-2 block">{label}</span><Input {...props} /></label> }
