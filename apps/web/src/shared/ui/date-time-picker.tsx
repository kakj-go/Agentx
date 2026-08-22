import * as Popover from '@radix-ui/react-popover'
import { CalendarClock } from 'lucide-react'
import { useMemo, useState } from 'react'
import { DayPicker } from 'react-day-picker'
import { enUS, zhCN } from 'react-day-picker/locale'
import { useTranslation } from 'react-i18next'

import { cn } from '../lib/cn'
import { resolveDisplayLocale } from '../lib/locale-format'
import { Button } from './button'
import { Input } from './input'

type DateTimePickerProps = {
  value?: Date
  onChange: (value?: Date) => void
  label: string
  messages: {
    time: string
    now: string
    clear: string
    confirm: string
    invalidTime: string
    outOfRange: string
  }
  min?: Date
  max?: Date
  className?: string
  disabled?: boolean
}

export function DateTimePicker({ value, onChange, label, messages, min, max, className, disabled }: DateTimePickerProps) {
  const { i18n } = useTranslation()
  const localeName = resolveDisplayLocale(i18n.resolvedLanguage)
  const locale = localeName === 'en-US' ? enUS : zhCN
  const [open, setOpen] = useState(false)
  const [selectedDate, setSelectedDate] = useState<Date>()
  const [displayMonth, setDisplayMonth] = useState(() => validDate(value) ?? new Date())
  const [time, setTime] = useState('00:00')
  const [error, setError] = useState('')
  const currentValue = validDate(value)
  const formatter = useMemo(() => new Intl.DateTimeFormat(localeName, {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    hourCycle: 'h23',
  }), [localeName])

  const initializeDraft = () => {
    const initial = validDate(value)
    setSelectedDate(initial)
    setDisplayMonth(initial ?? new Date())
    setTime(formatTime(initial ?? new Date()))
    setError('')
  }
  const setOpenState = (next: boolean) => {
    if (next) initializeDraft()
    setOpen(next)
  }
  const chooseNow = () => {
    const now = new Date()
    setSelectedDate(now)
    setDisplayMonth(now)
    setTime(formatTime(now))
    setError('')
  }
  const confirm = () => {
    if (!selectedDate) return
    const next = combineLocalDateTime(selectedDate, time)
    if (!next) {
      setError(messages.invalidTime)
      return
    }
    if ((validDate(min) && next < min!) || (validDate(max) && next > max!)) {
      setError(messages.outOfRange)
      return
    }
    onChange(next)
    setOpen(false)
  }
  const disabledDays = [
    ...(validDate(min) ? [{ before: startOfLocalDay(min!) }] : []),
    ...(validDate(max) ? [{ after: startOfLocalDay(max!) }] : []),
  ]

  return (
    <Popover.Root onOpenChange={setOpenState} open={open}>
      <Popover.Trigger asChild>
        <button
          aria-label={label}
          className={cn(
            'inline-flex h-9 min-w-44 items-center justify-between gap-2 rounded-lg border border-border bg-surface px-3 text-left text-xs text-foreground outline-none transition-colors',
            'hover:bg-muted/45 focus-visible:border-primary/60 focus-visible:ring-2 focus-visible:ring-primary/15 disabled:cursor-not-allowed disabled:opacity-50',
            !currentValue && 'text-muted-foreground',
            className,
          )}
          disabled={disabled}
          type="button"
        >
          <span className="truncate">{currentValue ? formatter.format(currentValue) : label}</span>
          <CalendarClock aria-hidden="true" className="size-4 shrink-0 text-muted-foreground" />
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          align="start"
          className="z-[230] w-[320px] rounded-xl border border-border bg-surface p-4 text-foreground shadow-xl outline-none"
          collisionPadding={16}
          sideOffset={6}
        >
          <DayPicker
            classNames={calendarClassNames}
            disabled={disabledDays}
            locale={locale}
            mode="single"
            month={displayMonth}
            onMonthChange={setDisplayMonth}
            onSelect={(date) => { setSelectedDate(date); setError('') }}
            selected={selectedDate}
            showOutsideDays
          />
          <div className="mt-3 border-t border-border pt-3">
            <label className="flex items-center justify-between gap-3 text-xs font-medium text-muted-foreground">
              <span>{messages.time}</span>
              <Input
                aria-invalid={Boolean(error)}
                aria-label={messages.time}
                className="w-24 text-center font-mono text-sm tracking-wider"
                inputMode="numeric"
                maxLength={5}
                onChange={(event) => { setTime(event.target.value); setError('') }}
                placeholder="HH:mm"
                value={time}
              />
            </label>
            {error && <p className="mt-2 text-xs text-danger" role="alert">{error}</p>}
          </div>
          <div className="mt-3 flex items-center justify-between gap-2">
            <Button onClick={() => { onChange(undefined); setOpen(false) }} size="sm" type="button" variant="ghost">
              {messages.clear}
            </Button>
            <div className="flex gap-2">
              <Button onClick={chooseNow} size="sm" type="button" variant="secondary">{messages.now}</Button>
              <Button disabled={!selectedDate} onClick={confirm} size="sm" type="button">{messages.confirm}</Button>
            </div>
          </div>
          <Popover.Arrow className="fill-border" />
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  )
}

const calendarClassNames = {
  root: 'relative w-full select-none',
  months: 'w-full',
  month: 'w-full space-y-2',
  month_caption: 'relative flex h-9 items-center justify-center',
  caption_label: 'text-sm font-semibold text-foreground',
  nav: 'absolute inset-x-0 top-0 z-10 flex items-center justify-between',
  button_previous: 'inline-flex size-9 items-center justify-center rounded-lg text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-primary/25 disabled:opacity-30',
  button_next: 'inline-flex size-9 items-center justify-center rounded-lg text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-primary/25 disabled:opacity-30',
  chevron: 'size-4 fill-current',
  month_grid: 'w-full border-collapse',
  weekdays: 'border-0',
  weekday: 'h-8 text-center text-[11px] font-medium text-muted-foreground',
  week: 'border-0',
  day: 'size-9 p-0 text-center text-xs text-foreground',
  day_button: 'inline-flex size-9 items-center justify-center rounded-lg outline-none transition-colors hover:bg-muted focus-visible:ring-2 focus-visible:ring-primary/30',
  selected: '[&>button]:bg-primary [&>button]:font-semibold [&>button]:text-primary-foreground [&>button]:hover:bg-primary/90',
  today: '[&>button]:ring-1 [&>button]:ring-inset [&>button]:ring-primary/45',
  outside: '[&>button]:text-muted-foreground/45',
  disabled: '[&>button]:pointer-events-none [&>button]:opacity-30',
  hidden: 'invisible',
}

function validDate(value?: Date) {
  return value && !Number.isNaN(value.getTime()) ? value : undefined
}

function formatTime(value: Date) {
  return `${String(value.getHours()).padStart(2, '0')}:${String(value.getMinutes()).padStart(2, '0')}`
}

function combineLocalDateTime(day: Date, time: string) {
  const match = /^(\d{2}):(\d{2})$/.exec(time)
  if (!match) return undefined
  const hours = Number(match[1])
  const minutes = Number(match[2])
  if (hours > 23 || minutes > 59) return undefined
  const result = new Date(day.getFullYear(), day.getMonth(), day.getDate(), hours, minutes, 0, 0)
  if (result.getHours() !== hours || result.getMinutes() !== minutes) return undefined
  return result
}

function startOfLocalDay(value: Date) {
  return new Date(value.getFullYear(), value.getMonth(), value.getDate())
}
