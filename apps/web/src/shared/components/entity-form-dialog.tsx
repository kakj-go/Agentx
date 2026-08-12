import { useEffect, useRef, useState, type FormEvent } from 'react'

import { ApiClientError, apiFieldErrors } from '../api/client'
import { Button } from '../ui/button'
import { Dialog, DialogContent } from '../ui/dialog'
import { Input } from '../ui/input'
import { Select } from '../ui/select'
import { Textarea } from '../ui/textarea'

export type EntityFormField = {
  name: string
  label: string
  type?: 'text' | 'password' | 'textarea' | 'select' | 'number'
  options?: Array<{ value: string; label: string }>
  defaultValue?: string
  max?: number
  min?: number
  placeholder?: string
  required?: boolean
  step?: number | 'any'
}

type Props = {
  cancelLabel: string
  fields: EntityFormField[]
  onClose: () => void
  onSubmit: (values: Record<string, string>) => Promise<void>
  open: boolean
  submitLabel: string
  title: string
}

export function EntityFormDialog({ cancelLabel, fields, onClose, onSubmit, open, submitLabel, title }: Props) {
  const [values, setValues] = useState<Record<string, string>>(() => Object.fromEntries(fields.map((field) => [field.name, field.defaultValue ?? ''])))
  const [error, setError] = useState('')
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})
  const [pending, setPending] = useState(false)
  const formRef = useRef<HTMLFormElement>(null)
  const fieldsRef = useRef(fields)
  fieldsRef.current = fields
  useEffect(() => {
    if (!open) return
    setValues(Object.fromEntries(fieldsRef.current.map((field) => [field.name, field.defaultValue ?? ''])))
    setError('')
    setFieldErrors({})
  }, [open])
  const update = (name: string, value: string) => {
    setValues((current) => ({ ...current, [name]: value }))
    setFieldErrors((current) => { const next = { ...current }; delete next[name]; return next })
  }
  const submit = async (event: FormEvent) => {
    event.preventDefault()
    setPending(true)
    setError('')
    setFieldErrors({})
    try { await onSubmit(values); onClose() } catch (value) {
      const fields = apiFieldErrors(value)
      setFieldErrors(fields)
      if (!Object.keys(fields).length || !(value instanceof ApiClientError)) setError(value instanceof Error ? value.message : String(value))
      requestAnimationFrame(() => formRef.current?.querySelector<HTMLElement>(`[name="${CSS.escape(Object.keys(fields)[0] ?? '')}"]`)?.focus())
    } finally { setPending(false) }
  }
  const wide = fields.length > 6
  return <Dialog onOpenChange={(value) => { if (!value) onClose() }} open={open}>
    <DialogContent className={wide ? 'w-[min(920px,calc(100vw-48px))] p-6' : 'p-6'} title={title}>
      <h2 className="text-lg font-semibold">{title}</h2>
      <form className={wide ? 'mt-5 grid grid-cols-2 gap-x-5 gap-y-4' : 'mt-5 space-y-4'} onSubmit={(event) => void submit(event)} ref={formRef}>
        {fields.map((field) => <label className={field.type === 'textarea' && wide ? 'col-span-2 block text-xs' : 'block text-xs'} key={field.name}>
          <span className="mb-2 block font-medium">{field.label}</span>
          {field.type === 'select' ? <Select aria-describedby={fieldErrors[field.name] ? `${field.name}-error` : undefined} aria-invalid={Boolean(fieldErrors[field.name])} className="w-full" name={field.name} onValueChange={(value) => update(field.name, value)} options={field.options ?? []} value={values[field.name]} />
            : field.type === 'textarea' ? <Textarea aria-describedby={fieldErrors[field.name] ? `${field.name}-error` : undefined} aria-invalid={Boolean(fieldErrors[field.name])} name={field.name} onChange={(event) => update(field.name, event.target.value)} placeholder={field.placeholder} required={field.required} value={values[field.name]} />
              : <Input aria-describedby={fieldErrors[field.name] ? `${field.name}-error` : undefined} aria-invalid={Boolean(fieldErrors[field.name])} max={field.max} min={field.type === 'number' ? (field.min ?? 0) : undefined} name={field.name} onChange={(event) => update(field.name, event.target.value)} placeholder={field.placeholder} required={field.required} step={field.step} type={field.type ?? 'text'} value={values[field.name]} />}
          {fieldErrors[field.name] && <span className="mt-1.5 block text-xs text-danger" id={`${field.name}-error`}>{fieldErrors[field.name]}</span>}
        </label>)}
        {error && <p className={wide ? 'col-span-2 rounded-lg border border-danger/20 bg-danger/10 p-3 text-xs text-danger' : 'rounded-lg border border-danger/20 bg-danger/10 p-3 text-xs text-danger'}>{error}</p>}
        <div className={wide ? 'col-span-2 flex justify-end gap-2' : 'flex justify-end gap-2'}><Button onClick={onClose} type="button" variant="ghost">{cancelLabel}</Button><Button disabled={pending} type="submit">{submitLabel}</Button></div>
      </form>
    </DialogContent>
  </Dialog>
}
